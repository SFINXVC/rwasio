use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{cell::RefCell, rc::Rc};

use libspa_sys as spa;
use pipewire::{context::ContextRc, core::PW_ID_CORE, main_loop::MainLoopRc, types::ObjectType};
use pipewire_sys as pw;

use crate::{
    ApplicationError, ApplicationResult,
    backends::{AudioBackend, AudioDevice, DuplexConfig, ProcessCallback},
};

unsafe extern "win64" {
    fn CreateThread(
        attrs: *const c_void,
        stack_size: usize,
        start: Option<unsafe extern "win64" fn(*mut c_void) -> u32>,
        param: *mut c_void,
        flags: u32,
        thread_id: *mut u32,
    ) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

const STACK_RESERVE_FLAG: u32 = 0x0001_0000;
const RT_STACK_BYTES: usize = 8 * 1024 * 1024;
const INFINITE: u32 = 0xFFFF_FFFF;
const RT_PRIO_MIN: i32 = 1;
const RT_PRIO_MAX: i32 = 80;
const THREAD_UTILS_TYPE: &CStr = c"Spa:Pointer:Interface:ThreadUtils";

const SPA_DIRECTION_INPUT: spa::spa_direction = 0;
const SPA_DIRECTION_OUTPUT: spa::spa_direction = 1;
const MAP_BUFFERS: pw::pw_filter_port_flags = 1;
const FILTER_FLAG_NONE: pw::pw_filter_flags = 0;

type SpaStart = unsafe extern "C" fn(*mut c_void) -> *mut c_void;

struct RtState {
    win_handle: *mut c_void,
    ptid: libc::pthread_t,
    ready: AtomicBool,
    rt_priority: i32,
    user_start: Option<SpaStart>,
    user_arg: *mut c_void,
    user_ret: *mut c_void,
}

fn flush_denormals() {
    let mut csr: u32 = 0;
    unsafe {
        core::arch::asm!("stmxcsr [{}]", in(reg) &mut csr, options(nostack, preserves_flags));
    }
    csr |= 0x8040;
    unsafe {
        core::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags));
    }
}

unsafe extern "win64" fn rt_trampoline(param: *mut c_void) -> u32 {
    let state = unsafe { &mut *(param as *mut RtState) };
    state.ptid = unsafe { libc::pthread_self() };
    state.ready.store(true, Ordering::Release);
    flush_denormals();
    if let Some(start) = state.user_start {
        state.user_ret = unsafe { start(state.user_arg) };
    }
    0
}

unsafe extern "C" fn rt_create(
    object: *mut c_void,
    _props: *const spa::spa_dict,
    start: Option<SpaStart>,
    arg: *mut c_void,
) -> *mut spa::spa_thread {
    let state = unsafe { &mut *(object as *mut RtState) };
    state.user_start = start;
    state.user_arg = arg;
    state.ready.store(false, Ordering::Relaxed);
    let handle = unsafe {
        CreateThread(
            core::ptr::null(),
            RT_STACK_BYTES,
            Some(rt_trampoline),
            object,
            STACK_RESERVE_FLAG,
            core::ptr::null_mut(),
        )
    };
    if handle.is_null() {
        return core::ptr::null_mut();
    }
    state.win_handle = handle;
    while !state.ready.load(Ordering::Acquire) {
        std::thread::yield_now();
    }
    state.ptid as *mut spa::spa_thread
}

unsafe extern "C" fn rt_join(
    object: *mut c_void,
    _thread: *mut spa::spa_thread,
    retval: *mut *mut c_void,
) -> i32 {
    let state = unsafe { &mut *(object as *mut RtState) };
    if state.win_handle.is_null() {
        return -1;
    }
    let waited = unsafe { WaitForSingleObject(state.win_handle, INFINITE) };
    if !retval.is_null() {
        unsafe { *retval = state.user_ret };
    }
    unsafe { CloseHandle(state.win_handle) };
    state.win_handle = core::ptr::null_mut();
    if waited == 0 { 0 } else { -1 }
}

unsafe extern "C" fn rt_get_rt_range(
    _object: *mut c_void,
    _props: *const spa::spa_dict,
    min: *mut i32,
    max: *mut i32,
) -> i32 {
    if !min.is_null() {
        unsafe { *min = RT_PRIO_MIN };
    }
    if !max.is_null() {
        unsafe { *max = RT_PRIO_MAX };
    }
    0
}

unsafe extern "C" fn rt_acquire_rt(
    object: *mut c_void,
    _thread: *mut spa::spa_thread,
    priority: i32,
) -> i32 {
    if priority <= 0 {
        return 0;
    }
    let state = unsafe { &mut *(object as *mut RtState) };
    let param = libc::sched_param {
        sched_priority: priority,
    };
    let err = unsafe { libc::pthread_setschedparam(state.ptid, libc::SCHED_FIFO, &param) };
    if err != 0 {
        return -1;
    }
    state.rt_priority = priority;
    0
}

unsafe extern "C" fn rt_drop_rt(object: *mut c_void, _thread: *mut spa::spa_thread) -> i32 {
    let state = unsafe { &mut *(object as *mut RtState) };
    if state.rt_priority == 0 {
        return 0;
    }
    let param = libc::sched_param { sched_priority: 0 };
    let err = unsafe { libc::pthread_setschedparam(state.ptid, libc::SCHED_OTHER, &param) };
    if err != 0 {
        return -1;
    }
    state.rt_priority = 0;
    0
}

static RT_METHODS: spa::spa_thread_utils_methods = spa::spa_thread_utils_methods {
    version: 0,
    create: Some(rt_create),
    join: Some(rt_join),
    get_rt_range: Some(rt_get_rt_range),
    acquire_rt: Some(rt_acquire_rt),
    drop_rt: Some(rt_drop_rt),
};

struct RtThreadUtils {
    _state: Box<RtState>,
    iface: Box<spa::spa_thread_utils>,
}

impl RtThreadUtils {
    fn new() -> Self {
        let mut state = Box::new(RtState {
            win_handle: core::ptr::null_mut(),
            ptid: 0,
            ready: AtomicBool::new(false),
            rt_priority: 0,
            user_start: None,
            user_arg: core::ptr::null_mut(),
            user_ret: core::ptr::null_mut(),
        });
        let data = state.as_mut() as *mut RtState as *mut c_void;
        let iface = Box::new(spa::spa_thread_utils {
            iface: spa::spa_interface {
                type_: THREAD_UTILS_TYPE.as_ptr(),
                version: 0,
                cb: spa::spa_callbacks {
                    funcs: &RT_METHODS as *const spa::spa_thread_utils_methods as *const c_void,
                    data,
                },
            },
        });
        Self {
            _state: state,
            iface,
        }
    }

    fn as_ptr(&self) -> *mut spa::spa_thread_utils {
        self.iface.as_ref() as *const spa::spa_thread_utils as *mut spa::spa_thread_utils
    }
}

struct FilterData {
    in_ports: Vec<*mut c_void>,
    out_ports: Vec<*mut c_void>,
    buffer_size: usize,
    process: ProcessCallback,
}

unsafe fn port_in_slice<'a>(b: *mut pw::pw_buffer, n: usize) -> &'a [f32] {
    let buf = unsafe { (*b).buffer };
    if buf.is_null() || unsafe { (*buf).n_datas } == 0 {
        return &[];
    }
    let d = unsafe { &*(*buf).datas };
    if d.data.is_null() {
        return &[];
    }
    let avail = if d.chunk.is_null() {
        n
    } else {
        (unsafe { (*d.chunk).size } as usize) / 4
    };
    let count = avail.min(n);
    unsafe { core::slice::from_raw_parts(d.data as *const f32, count) }
}

unsafe fn port_out_slice<'a>(b: *mut pw::pw_buffer, n: usize) -> &'a mut [f32] {
    let buf = unsafe { (*b).buffer };
    if buf.is_null() || unsafe { (*buf).n_datas } == 0 {
        return &mut [];
    }
    let d = unsafe { &*(*buf).datas };
    if d.data.is_null() {
        return &mut [];
    }
    let count = ((d.maxsize as usize) / 4).min(n);
    unsafe { core::slice::from_raw_parts_mut(d.data as *mut f32, count) }
}

unsafe fn set_out_chunk(b: *mut pw::pw_buffer, n: usize) {
    let buf = unsafe { (*b).buffer };
    if buf.is_null() || unsafe { (*buf).n_datas } == 0 {
        return;
    }
    let d = unsafe { &*(*buf).datas };
    if d.chunk.is_null() {
        return;
    }
    let bytes = ((n * 4) as u32).min(d.maxsize);
    unsafe {
        (*d.chunk).offset = 0;
        (*d.chunk).size = bytes;
        (*d.chunk).stride = 4;
        (*d.chunk).flags = 0;
        (*b).size = n as u64;
    }
}

unsafe extern "C" fn on_process(data: *mut c_void, _position: *mut spa::spa_io_position) {
    if data.is_null() {
        return;
    }
    let fd = unsafe { &mut *(data as *mut FilterData) };
    let n = fd.buffer_size;

    let mut in_bufs: Vec<*mut pw::pw_buffer> = Vec::with_capacity(fd.in_ports.len());
    let mut inputs: Vec<&[f32]> = Vec::with_capacity(fd.in_ports.len());
    for &p in &fd.in_ports {
        let b = unsafe { pw::pw_filter_dequeue_buffer(p) };
        in_bufs.push(b);
        if b.is_null() {
            inputs.push(&[]);
        } else {
            inputs.push(unsafe { port_in_slice(b, n) });
        }
    }

    let mut out_bufs: Vec<*mut pw::pw_buffer> = Vec::with_capacity(fd.out_ports.len());
    let mut outputs: Vec<&mut [f32]> = Vec::with_capacity(fd.out_ports.len());
    for &p in &fd.out_ports {
        let b = unsafe { pw::pw_filter_dequeue_buffer(p) };
        out_bufs.push(b);
        if b.is_null() {
            outputs.push(&mut []);
        } else {
            outputs.push(unsafe { port_out_slice(b, n) });
        }
    }

    (fd.process)(&inputs, &mut outputs);

    for (i, &b) in out_bufs.iter().enumerate() {
        if b.is_null() {
            continue;
        }
        unsafe {
            set_out_chunk(b, n);
            pw::pw_filter_queue_buffer(fd.out_ports[i], b);
        }
    }
    for (i, &b) in in_bufs.iter().enumerate() {
        if b.is_null() {
            continue;
        }
        unsafe { pw::pw_filter_queue_buffer(fd.in_ports[i], b) };
    }
}

static FILTER_EVENTS: pw::pw_filter_events = pw::pw_filter_events {
    version: pw::PW_VERSION_FILTER_EVENTS,
    destroy: None,
    state_changed: None,
    io_changed: None,
    param_changed: None,
    add_buffer: None,
    remove_buffer: None,
    process: Some(on_process),
    drained: None,
    command: None,
};

struct RunningStream {
    loop_: *mut pw::pw_thread_loop,
    ctx: *mut pw::pw_context,
    core: *mut pw::pw_core,
    filter: *mut pw::pw_filter,
    _rt: RtThreadUtils,
    _data: Box<FilterData>,
}

unsafe impl Send for RunningStream {}

static INIT: Once = Once::new();

fn ensure_init() {
    INIT.call_once(|| unsafe { pw::pw_init(core::ptr::null_mut(), core::ptr::null_mut()) });
}

fn err(msg: &str) -> ApplicationError {
    ApplicationError::Unknown(msg.to_string())
}

fn props_new() -> ApplicationResult<*mut pw::pw_properties> {
    let p = unsafe { pw::pw_properties_new(core::ptr::null(), core::ptr::null::<c_char>()) };
    if p.is_null() {
        return Err(err("pw_properties_new failed"));
    }
    Ok(p)
}

fn props_set(p: *mut pw::pw_properties, key: &CStr, val: &str) -> ApplicationResult<()> {
    let cval = CString::new(val).map_err(|_| err("property value had interior NUL"))?;
    unsafe { pw::pw_properties_set(p, key.as_ptr(), cval.as_ptr()) };
    Ok(())
}

fn node_props(config: &DuplexConfig) -> ApplicationResult<*mut pw::pw_properties> {
    let rate = config.sample_rate as u32;
    let p = props_new()?;
    props_set(p, c"node.name", "rwasio")?;
    props_set(p, c"media.type", "Audio")?;
    props_set(p, c"media.category", "Duplex")?;
    props_set(p, c"media.role", "DSP")?;
    props_set(p, c"node.always-process", "true")?;
    props_set(p, c"node.force-quantum", &config.buffer_size.to_string())?;
    props_set(p, c"node.force-rate", &rate.to_string())?;
    props_set(
        p,
        c"node.latency",
        &format!("{}/{}", config.buffer_size, rate),
    )?;
    Ok(p)
}

fn port_props(name: &str) -> ApplicationResult<*mut pw::pw_properties> {
    let p = props_new()?;
    props_set(p, c"format.dsp", "32 bit float mono audio")?;
    props_set(p, c"port.name", name)?;
    Ok(p)
}

struct PortInfo {
    id: u32,
    node: u32,
    is_output: bool,
    name: String,
}

struct NodeInfo {
    id: u32,
    class: String,
    name: String,
}

fn json_name(v: &str) -> Option<String> {
    let i = v.find("\"name\"")?;
    let rest = &v[i + 6..];
    let q1 = rest.find('"')?;
    let after = &rest[q1 + 1..];
    let q2 = after.find('"')?;
    Some(after[..q2].to_string())
}

fn drain_sync(main_loop: &MainLoopRc, core: &pipewire::core::CoreRc) -> ApplicationResult<()> {
    let ml = main_loop.clone();
    let pending = core.sync(0)?;
    let _done = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == PW_ID_CORE && seq == pending {
                ml.quit();
            }
        })
        .register();
    main_loop.run();
    Ok(())
}

#[tracing::instrument(skip_all, fields(out_target = ?config.output_target, in_target = ?config.input_target))]
fn connect_ports(config: &DuplexConfig) -> ApplicationResult<()> {
    pipewire::init();

    let main_loop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;

    let nodes: Rc<RefCell<Vec<NodeInfo>>> = Rc::new(RefCell::new(Vec::new()));
    let ports: Rc<RefCell<Vec<PortInfo>>> = Rc::new(RefCell::new(Vec::new()));
    let default_sink: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let default_source: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    type MetaHold = Vec<(
        pipewire::metadata::Metadata,
        pipewire::metadata::MetadataListener,
    )>;
    let meta_hold: Rc<RefCell<MetaHold>> = Rc::new(RefCell::new(Vec::new()));

    let nodes_cb = nodes.clone();
    let ports_cb = ports.clone();
    let default_sink_cb = default_sink.clone();
    let default_source_cb = default_source.clone();
    let meta_hold_cb = meta_hold.clone();
    let registry_cb = registry.clone();

    let _reg_listener = registry
        .add_listener_local()
        .global(move |obj| {
            let Some(props) = &obj.props else { return };
            match obj.type_ {
                ObjectType::Node => {
                    nodes_cb.borrow_mut().push(NodeInfo {
                        id: obj.id,
                        class: props.get("media.class").unwrap_or("").to_string(),
                        name: props.get("node.name").unwrap_or("").to_string(),
                    });
                }
                ObjectType::Port => {
                    let Some(node) = props.get("node.id").and_then(|s| s.parse::<u32>().ok())
                    else {
                        return;
                    };
                    ports_cb.borrow_mut().push(PortInfo {
                        id: obj.id,
                        node,
                        is_output: props.get("port.direction") == Some("out"),
                        name: props.get("port.name").unwrap_or("").to_string(),
                    });
                }
                ObjectType::Metadata => {
                    if props.get("metadata.name") != Some("default")
                        || !meta_hold_cb.borrow().is_empty()
                    {
                        return;
                    }
                    let Ok(meta) = registry_cb.bind::<pipewire::metadata::Metadata, _>(obj) else {
                        return;
                    };
                    let ds = default_sink_cb.clone();
                    let dsrc = default_source_cb.clone();
                    let listener = meta
                        .add_listener_local()
                        .property(move |_subject, key, _type, value| {
                            if let (Some(k), Some(v)) = (key, value) {
                                if k == "default.audio.sink" {
                                    *ds.borrow_mut() = json_name(v);
                                } else if k == "default.audio.source" {
                                    *dsrc.borrow_mut() = json_name(v);
                                }
                            }
                            0
                        })
                        .register();
                    meta_hold_cb.borrow_mut().push((meta, listener));
                }
                _ => {}
            }
        })
        .register();

    let mut our_node = None;
    for _ in 0..30 {
        drain_sync(&main_loop, &core)?;
        our_node = nodes
            .borrow()
            .iter()
            .find(|n| n.name == "rwasio")
            .map(|n| n.id);
        if let Some(nid) = our_node
            && ports.borrow().iter().any(|p| p.node == nid)
        {
            break;
        }
        our_node = None;
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let Some(our_node) = our_node else {
        return Err(err("filter node not visible in registry; not linking"));
    };

    if config.output_target.is_none() || config.input_target.is_none() {
        for _ in 0..15 {
            if default_sink.borrow().is_some() && default_source.borrow().is_some() {
                break;
            }
            drain_sync(&main_loop, &core)?;
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    let nodes_b = nodes.borrow();
    let ports_b = ports.borrow();

    let collect = |node: u32, output: bool| -> Vec<(u32, String)> {
        let mut v: Vec<(u32, String)> = ports_b
            .iter()
            .filter(|p| p.node == node && p.is_output == output)
            .map(|p| (p.id, p.name.clone()))
            .collect();
        v.sort_by(|a, b| a.1.cmp(&b.1));
        v
    };

    let our_out = collect(our_node, true);
    let our_in = collect(our_node, false);

    let resolve_default = |name: Option<String>, class: &str| -> Option<u32> {
        if let Some(n) = name
            && let Some(id) = nodes_b.iter().find(|x| x.name == n).map(|x| x.id)
        {
            return Some(id);
        }
        nodes_b.iter().find(|x| x.class == class).map(|x| x.id)
    };

    let sink_node = config
        .output_target
        .or_else(|| resolve_default(default_sink.borrow().clone(), "Audio/Sink"));
    let source_node = config
        .input_target
        .or_else(|| resolve_default(default_source.borrow().clone(), "Audio/Source"));

    let mk = |out_node: u32,
              out_port: u32,
              in_node: u32,
              in_port: u32|
     -> ApplicationResult<pipewire::link::Link> {
        let mut props = pipewire::properties::PropertiesBox::new();
        props.insert(*pipewire::keys::OBJECT_LINGER, "true");
        props.insert(*pipewire::keys::LINK_OUTPUT_NODE, out_node.to_string());
        props.insert(*pipewire::keys::LINK_OUTPUT_PORT, out_port.to_string());
        props.insert(*pipewire::keys::LINK_INPUT_NODE, in_node.to_string());
        props.insert(*pipewire::keys::LINK_INPUT_PORT, in_port.to_string());
        core.create_object::<pipewire::link::Link>("link-factory", &props)
            .map_err(|e| err(&format!("link-factory failed: {e}")))
    };

    let mut links: Vec<pipewire::link::Link> = Vec::new();

    if let Some(sink) = sink_node {
        let sink_in = collect(sink, false);
        for (o, s) in our_out.iter().zip(sink_in.iter()) {
            links.push(mk(our_node, o.0, sink, s.0)?);
        }
    }
    if let Some(source) = source_node {
        let src_out = collect(source, true);
        for (c, i) in src_out.iter().zip(our_in.iter()) {
            links.push(mk(source, c.0, our_node, i.0)?);
        }
    }

    drain_sync(&main_loop, &core)?;
    tracing::info!(
        "linked {} ports (node={}, sink={:?}, source={:?})",
        links.len(),
        our_node,
        sink_node,
        source_node
    );
    Ok(())
}

type DeviceMap = std::collections::HashMap<u32, (bool, String)>;

fn publish_devices(map: &DeviceMap) {
    let mut sinks: Vec<(String, String)> = map
        .iter()
        .filter(|(_, (is_sink, _))| *is_sink)
        .map(|(id, (_, name))| (name.clone(), id.to_string()))
        .collect();
    let mut sources: Vec<(String, String)> = map
        .iter()
        .filter(|(_, (is_sink, _))| !*is_sink)
        .map(|(id, (_, name))| (name.clone(), id.to_string()))
        .collect();
    sinks.sort();
    sources.sort();
    if let Ok(mut dl) = crate::DEVICE_LIST.write() {
        *dl = (sinks, sources);
    }
    crate::DEVICE_GENERATION.fetch_add(1, Ordering::Relaxed);
}

fn run_device_monitor() -> ApplicationResult<()> {
    pipewire::init();

    let main_loop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;

    let devices: Rc<RefCell<DeviceMap>> = Rc::new(RefCell::new(DeviceMap::new()));
    let add = devices.clone();
    let remove = devices.clone();

    let _listener = registry
        .add_listener_local()
        .global(move |obj| {
            if obj.type_ != ObjectType::Node {
                return;
            }
            let Some(props) = &obj.props else { return };
            let Some(class) = props.get("media.class") else {
                return;
            };
            let is_sink = match class {
                "Audio/Sink" => true,
                "Audio/Source" => false,
                _ => return,
            };
            let name = props
                .get("node.description")
                .or_else(|| props.get("node.name"))
                .unwrap_or("Unknown")
                .to_string();
            add.borrow_mut().insert(obj.id, (is_sink, name));
            publish_devices(&add.borrow());
        })
        .global_remove(move |id| {
            if remove.borrow_mut().remove(&id).is_some() {
                publish_devices(&remove.borrow());
            }
        })
        .register();

    main_loop.run();
    Ok(())
}

pub fn spawn_device_monitor() {
    std::thread::spawn(|| {
        if let Err(e) = run_device_monitor() {
            tracing::warn!("device monitor stopped: {e}");
        }
    });
}

pub fn enumerate_devices() -> ApplicationResult<(Vec<AudioDevice>, Vec<AudioDevice>)> {
    pipewire::init();

    let main_loop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;

    let sinks: Rc<RefCell<Vec<AudioDevice>>> = Rc::new(RefCell::new(vec![]));
    let sources: Rc<RefCell<Vec<AudioDevice>>> = Rc::new(RefCell::new(vec![]));

    let sinks_clone = sinks.clone();
    let sources_clone = sources.clone();

    let _reg_listener = registry
        .add_listener_local()
        .global(move |object| {
            if object.type_ != ObjectType::Node {
                return;
            }

            let Some(props) = &object.props else { return };
            let Some(class) = props.get("media.class") else {
                return;
            };

            let name = props
                .get("node.description")
                .or_else(|| props.get("node.name"))
                .unwrap_or("Unknown")
                .to_string();

            let id = object.id.to_string();

            match class {
                "Audio/Sink" => sinks_clone.borrow_mut().push(AudioDevice { name, id }),
                "Audio/Source" => sources_clone.borrow_mut().push(AudioDevice { name, id }),
                _ => {}
            }
        })
        .register();

    drain_sync(&main_loop, &core)?;

    let sinks = sinks.borrow().clone();
    let sources = sources.borrow().clone();
    Ok((sinks, sources))
}

pub struct PipeWireAudioBackend {
    sinks: Vec<AudioDevice>,
    sources: Vec<AudioDevice>,
    running: Option<RunningStream>,
}

impl PipeWireAudioBackend {
    pub const NAME: &'static str = "PipeWire";

    pub fn new() -> ApplicationResult<Self> {
        let (sinks, sources) = enumerate_devices()?;
        Ok(Self {
            sinks,
            sources,
            running: None,
        })
    }

    #[tracing::instrument(skip_all, fields(buffer = config.buffer_size, inputs = config.input_channels, outputs = config.output_channels))]
    fn build_stream(
        &mut self,
        config: DuplexConfig,
        process: ProcessCallback,
    ) -> ApplicationResult<RunningStream> {
        ensure_init();

        let mut data = Box::new(FilterData {
            in_ports: Vec::new(),
            out_ports: Vec::new(),
            buffer_size: config.buffer_size as usize,
            process,
        });
        let data_ptr = data.as_mut() as *mut FilterData as *mut c_void;

        let name = CString::new("rwasio").map_err(|_| err("name had interior NUL"))?;

        let loop_ = unsafe { pw::pw_thread_loop_new(name.as_ptr(), core::ptr::null()) };
        if loop_.is_null() {
            return Err(err("pw_thread_loop_new failed"));
        }
        let ctx = unsafe {
            pw::pw_context_new(pw::pw_thread_loop_get_loop(loop_), core::ptr::null_mut(), 0)
        };
        if ctx.is_null() {
            unsafe { pw::pw_thread_loop_destroy(loop_) };
            return Err(err("pw_context_new failed"));
        }

        let data_loop = unsafe { pw::pw_context_get_data_loop(ctx) };
        let rt = RtThreadUtils::new();
        unsafe {
            pw::pw_data_loop_set_thread_utils(data_loop, rt.as_ptr());
            pw::pw_data_loop_stop(data_loop);
        }

        if unsafe { pw::pw_thread_loop_start(loop_) } < 0 {
            unsafe {
                pw::pw_context_destroy(ctx);
                pw::pw_thread_loop_destroy(loop_);
            }
            return Err(err("pw_thread_loop_start failed"));
        }

        unsafe { pw::pw_thread_loop_lock(loop_) };

        let core = unsafe { pw::pw_context_connect(ctx, core::ptr::null_mut(), 0) };
        if core.is_null() {
            unsafe {
                pw::pw_thread_loop_unlock(loop_);
                pw::pw_thread_loop_stop(loop_);
                pw::pw_context_destroy(ctx);
                pw::pw_thread_loop_destroy(loop_);
            }
            return Err(err("pw_context_connect failed (is the daemon running?)"));
        }

        let nprops = match node_props(&config) {
            Ok(p) => p,
            Err(e) => {
                unsafe {
                    pw::pw_thread_loop_unlock(loop_);
                    pw::pw_thread_loop_stop(loop_);
                    pw::pw_context_destroy(ctx);
                    pw::pw_thread_loop_destroy(loop_);
                }
                return Err(e);
            }
        };

        let filter = unsafe {
            pw::pw_filter_new_simple(
                pw::pw_data_loop_get_loop(data_loop),
                name.as_ptr(),
                nprops,
                &FILTER_EVENTS,
                data_ptr,
            )
        };
        if filter.is_null() {
            unsafe {
                pw::pw_thread_loop_unlock(loop_);
                pw::pw_thread_loop_stop(loop_);
                pw::pw_context_destroy(ctx);
                pw::pw_thread_loop_destroy(loop_);
            }
            return Err(err("pw_filter_new_simple failed"));
        }

        for i in 0..config.input_channels {
            let pp = port_props(&format!("in_{}", i + 1))?;
            let handle = unsafe {
                pw::pw_filter_add_port(
                    filter,
                    SPA_DIRECTION_INPUT,
                    MAP_BUFFERS,
                    0,
                    pp,
                    core::ptr::null_mut(),
                    0,
                )
            };
            if handle.is_null() {
                return Err(err("pw_filter_add_port (input) failed"));
            }
            data.in_ports.push(handle);
        }
        for i in 0..config.output_channels {
            let pp = port_props(&format!("out_{}", i + 1))?;
            let handle = unsafe {
                pw::pw_filter_add_port(
                    filter,
                    SPA_DIRECTION_OUTPUT,
                    MAP_BUFFERS,
                    0,
                    pp,
                    core::ptr::null_mut(),
                    0,
                )
            };
            if handle.is_null() {
                return Err(err("pw_filter_add_port (output) failed"));
            }
            data.out_ports.push(handle);
        }

        if unsafe { pw::pw_filter_connect(filter, FILTER_FLAG_NONE, core::ptr::null_mut(), 0) } < 0
        {
            unsafe { pw::pw_thread_loop_unlock(loop_) };
            return Err(err("pw_filter_connect failed"));
        }

        unsafe {
            pw::pw_thread_loop_unlock(loop_);
            pw::pw_thread_loop_lock(loop_);
            let started = pw::pw_data_loop_start(data_loop);
            pw::pw_thread_loop_unlock(loop_);
            if started < 0 {
                return Err(err("pw_data_loop_start failed"));
            }
        }

        tracing::info!(
            "running: {} in / {} out @ {} frames",
            config.input_channels,
            config.output_channels,
            config.buffer_size
        );

        if let Err(e) = connect_ports(&config) {
            tracing::warn!("linking failed: {e}");
        }

        Ok(RunningStream {
            loop_,
            ctx,
            core,
            filter,
            _rt: rt,
            _data: data,
        })
    }
}

impl AudioBackend for PipeWireAudioBackend {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn sinks(&self) -> Vec<AudioDevice> {
        self.sinks.clone()
    }

    fn sources(&self) -> Vec<AudioDevice> {
        self.sources.clone()
    }

    fn start(&mut self, config: DuplexConfig, process: ProcessCallback) -> ApplicationResult<()> {
        if self.running.is_some() {
            return Ok(());
        }
        let stream = self.build_stream(config, process)?;
        self.running = Some(stream);
        Ok(())
    }

    fn stop(&mut self) -> ApplicationResult<()> {
        if let Some(r) = self.running.take() {
            unsafe {
                pw::pw_thread_loop_lock(r.loop_);
                pw::pw_data_loop_stop(pw::pw_context_get_data_loop(r.ctx));
                pw::pw_filter_destroy(r.filter);
                pw::pw_thread_loop_unlock(r.loop_);
                pw::pw_core_disconnect(r.core);
                pw::pw_thread_loop_stop(r.loop_);
                pw::pw_context_destroy(r.ctx);
                pw::pw_thread_loop_destroy(r.loop_);
            }
        }
        Ok(())
    }

    fn set_output_target(&mut self, id: Option<u32>) -> ApplicationResult<()> {
        tracing::info!("set_output_target {:?} (applies on restart)", id);
        Ok(())
    }

    fn set_input_target(&mut self, id: Option<u32>) -> ApplicationResult<()> {
        tracing::info!("set_input_target {:?} (applies on restart)", id);
        Ok(())
    }
}
