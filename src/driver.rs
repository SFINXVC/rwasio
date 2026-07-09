use crate::backends::AudioBackend;
use crate::backends::pipewire::PipeWireAudioBackend;
use crate::com::AsioClass;
use crate::{DEVICE_LIST, asio::*};
use core::ffi::c_char;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};

unsafe extern "win64" {
    fn CreateThread(
        lp_thread_attributes: *const core::ffi::c_void,
        dw_stack_size: usize,
        lp_start_address: Option<unsafe extern "win64" fn(*mut core::ffi::c_void) -> u32>,
        lp_parameter: *mut core::ffi::c_void,
        dw_creation_flags: u32,
        lp_thread_id: *mut u32,
    ) -> *mut core::ffi::c_void;

    fn Sleep(dw_milliseconds: u32);
}

pub struct RWAsioDriver {
    pub num_inputs: i32,
    pub num_outputs: i32,
    pub sample_rate: SampleRate,
    pub buffer_size_min: i32,
    pub buffer_size_max: i32,
    pub buffer_size_preferred: i32,
    pub buffer_size_granularity: i32,
    initialized: bool,
    running: bool,
    error_message: String,
    buffers: Vec<Box<[u8]>>,
}

static PREFERRED_BUFFER_SIZE: AtomicI32 = AtomicI32::new(BUFFER_SIZE_PREFERRED);
static ASIO_BUFFER_FRAMES: AtomicI32 = AtomicI32::new(0);
static ASIO_MESSAGE_FN: AtomicUsize = AtomicUsize::new(0);
static RESET_PENDING: AtomicBool = AtomicBool::new(false);
static RESET_WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static MONITOR_STARTED: AtomicBool = AtomicBool::new(false);
static BUFFER_SWITCH_FN: AtomicUsize = AtomicUsize::new(0);
static BUFFER_SWITCH_TIME_INFO_FN: AtomicUsize = AtomicUsize::new(0);
static HOST_WANTS_TIME_INFO: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
static CURRENT_BUFFER_INDEX: AtomicI32 = AtomicI32::new(0);
static SAMPLE_POSITION: AtomicU64 = AtomicU64::new(0);
static ASIO_OUTPUT_PTRS: std::sync::Mutex<Vec<[usize; 2]>> = std::sync::Mutex::new(Vec::new());
static ASIO_INPUT_PTRS: std::sync::Mutex<Vec<[usize; 2]>> = std::sync::Mutex::new(Vec::new());

fn parse_target(node_id: &str) -> Option<u32> {
    if node_id.is_empty() {
        None
    } else {
        node_id.parse().ok()
    }
}

pub fn set_output_target(node_id: &str) {
    RESET_PENDING.store(true, Ordering::Relaxed);
    tracing::info!("set_output_target id={:?}, reset pending", node_id);
}

pub fn set_input_target(node_id: &str) {
    RESET_PENDING.store(true, Ordering::Relaxed);
    tracing::info!("set_input_target id={:?}, reset pending", node_id);
}

pub fn request_host_reset() {
    RESET_PENDING.store(true, Ordering::Relaxed);
}

pub fn preferred_buffer_size() -> i32 {
    PREFERRED_BUFFER_SIZE.load(Ordering::Relaxed)
}

pub fn set_preferred_buffer_size(size: i32) {
    PREFERRED_BUFFER_SIZE.store(size, Ordering::Relaxed);
    RESET_PENDING.store(true, Ordering::Relaxed);
    tracing::info!("preferred buffer size -> {}, reset pending", size);
}

unsafe extern "win64" fn reset_worker(_: *mut core::ffi::c_void) -> u32 {
    loop {
        unsafe {
            Sleep(50);
        }
        fire_reset_if_pending();
    }
}

fn fire_reset_if_pending() {
    if RESET_PENDING.swap(false, Ordering::Relaxed) {
        let ptr = ASIO_MESSAGE_FN.load(Ordering::Relaxed);
        if ptr != 0 {
            let f: unsafe extern "win64" fn(i32, i32, *mut core::ffi::c_void, *mut f64) -> i32 =
                unsafe { core::mem::transmute(ptr) };
            unsafe {
                f(
                    MessageSelector::ResetRequest as i32,
                    1,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                );
            }
            tracing::info!("sent ResetRequest to host");
        }
    }
}

const DRIVER_VERSION: i32 = 1;
const DEFAULT_INPUTS: i32 = 2;
const DEFAULT_OUTPUTS: i32 = 2;
const DEFAULT_SAMPLE_RATE: SampleRate = 44_100.0;
const BUFFER_SIZE_MIN: i32 = 64;
const BUFFER_SIZE_MAX: i32 = 2_048;
const BUFFER_SIZE_PREFERRED: i32 = 256;
const BUFFER_SIZE_GRANULARITY: i32 = -1;
const SAMPLE_BYTES: usize = size_of::<f32>();

impl Default for RWAsioDriver {
    fn default() -> Self {
        Self {
            num_inputs: DEFAULT_INPUTS,
            num_outputs: DEFAULT_OUTPUTS,
            sample_rate: DEFAULT_SAMPLE_RATE,
            buffer_size_min: BUFFER_SIZE_MIN,
            buffer_size_max: BUFFER_SIZE_MAX,
            buffer_size_preferred: BUFFER_SIZE_PREFERRED,
            buffer_size_granularity: BUFFER_SIZE_GRANULARITY,
            initialized: false,
            running: false,
            error_message: String::new(),
            buffers: Vec::new(),
        }
    }
}

fn write_cstr(dst: &mut [c_char], src: &str) {
    dst.fill(0);
    let len = src.len().min(dst.len() - 1);

    for (out, b) in dst.iter_mut().take(len).zip(src.bytes()) {
        *out = b as c_char;
    }
}

fn samples_from_u64(value: u64) -> Samples {
    Samples {
        hi: (value >> 32) as u32,
        lo: value as u32,
    }
}

fn now_timestamp() -> TimeStamp {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    let ns = (ts.tv_sec as u64)
        .wrapping_mul(1_000_000_000)
        .wrapping_add(ts.tv_nsec as u64);
    TimeStamp {
        hi: (ns >> 32) as u32,
        lo: ns as u32,
    }
}

impl AsioClass for RWAsioDriver {
    const CLSID: Guid = crate::guid!("019eb112-f780-734f-ab6b-5b2ab7e81380");
    const NAME: &'static str = "Rusty Wine ASIO";
    const DESCRIPTION: &'static str = "Rusty Wine ASIO Driver";
    const DLL_FILE: &'static str = "rwasio.dll";

    fn new() -> Self {
        tracing::info!("new");

        Self::default()
    }
}

impl Asio for RWAsioDriver {
    #[tracing::instrument(skip_all)]
    fn init(&mut self, _sys_handle: usize) -> Bool {
        tracing::info!("init");
        self.initialized = true;

        if !RESET_WORKER_STARTED.swap(true, Ordering::Relaxed) {
            crate::MODULE_PINNED.store(true, Ordering::SeqCst);
            unsafe {
                CreateThread(
                    core::ptr::null(),
                    0,
                    Some(reset_worker),
                    core::ptr::null_mut(),
                    0,
                    core::ptr::null_mut(),
                );
            }
            tracing::info!("reset worker thread started");
        }

        if crate::ACTIVE_BACKEND.get().is_none() {
            match PipeWireAudioBackend::new() {
                Ok(v) => {
                    let sinks = v.sinks().into_iter().map(|d| (d.name, d.id)).collect();
                    let sources = v.sources().into_iter().map(|d| (d.name, d.id)).collect();
                    if let Ok(mut dl) = DEVICE_LIST.write() {
                        *dl = (sinks, sources);
                    }
                    crate::ACTIVE_BACKEND.get_or_init(|| std::sync::Mutex::new(Box::new(v)));
                    tracing::info!("device list populated");
                    if !MONITOR_STARTED.swap(true, Ordering::Relaxed) {
                        crate::MODULE_PINNED.store(true, Ordering::SeqCst);
                        crate::backends::pipewire::spawn_device_monitor();
                        tracing::info!("device monitor started");
                    }
                }
                Err(e) => {
                    tracing::error!("backend init failed: {}", e);
                    self.error_message = format!("PipeWire backend failed: {e}");
                    return Bool::FALSE;
                }
            }
        }

        if crate::ACTIVE_BACKEND.get().is_none() {
            return Bool::FALSE;
        }

        Bool::TRUE
    }

    fn get_driver_name(&self) -> &str {
        Self::NAME
    }

    fn get_driver_version(&self) -> i32 {
        DRIVER_VERSION
    }

    fn get_error_message(&self) -> &str {
        &self.error_message
    }

    #[tracing::instrument(skip_all, fields(inputs = self.num_inputs, outputs = self.num_outputs, rate = self.sample_rate))]
    fn start(&mut self) -> AsioResult<()> {
        if !self.initialized {
            return Err(AsioError::InvalidMode);
        }
        if self.running {
            return Ok(());
        }

        tracing::info!("start");

        RUNNING.store(true, Ordering::SeqCst);
        CURRENT_BUFFER_INDEX.store(0, Ordering::SeqCst);
        SAMPLE_POSITION.store(0, Ordering::SeqCst);

        let frames = ASIO_BUFFER_FRAMES.load(Ordering::SeqCst);
        if frames <= 0 {
            RUNNING.store(false, Ordering::SeqCst);
            return Err(AsioError::InvalidMode);
        }
        let buffer_size = frames as usize;

        let channels_in = self.num_inputs as usize;
        let channels_out = self.num_outputs as usize;
        let sample_rate = self.sample_rate;

        // Snapshot raw ASIO buffer pointers. create_buffers always runs before
        // start(), and dispose_buffers only runs after stop() joins the RT
        // thread, so these stay valid for the lifetime of the process closure.
        let input_ptrs: Vec<[usize; 2]> = ASIO_INPUT_PTRS
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let output_ptrs: Vec<[usize; 2]> = ASIO_OUTPUT_PTRS
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        let process: crate::backends::ProcessCallback = Box::new(
            move |inputs: &[crate::backends::InPort], outputs: &mut [crate::backends::OutPort]| {
                if !RUNNING.load(Ordering::Relaxed) {
                    for o in outputs.iter() {
                        if !o.ptr.is_null() {
                            unsafe { core::ptr::write_bytes(o.ptr, 0, o.len) };
                        }
                    }
                    return;
                }

                let idx = CURRENT_BUFFER_INDEX.load(Ordering::Relaxed) as usize;
                crate::DBG_CURRENT_BUFFER_IDX.store(idx as u32, Ordering::Relaxed);

                for ch in 0..channels_in {
                    let Some(src) = inputs.get(ch) else { continue };
                    if src.ptr.is_null() {
                        continue;
                    }
                    if let Some(&[p0, p1]) = input_ptrs.get(ch) {
                        let dst = if idx == 0 { p0 } else { p1 } as *mut f32;
                        if !dst.is_null() {
                            let n = src.len.min(buffer_size);
                            unsafe { core::ptr::copy_nonoverlapping(src.ptr, dst, n) };
                        }
                    }
                }

                let wants_time_info = HOST_WANTS_TIME_INFO.load(Ordering::Relaxed);
                let time_info_ptr = BUFFER_SWITCH_TIME_INFO_FN.load(Ordering::Relaxed);
                let position_before = SAMPLE_POSITION.load(Ordering::Relaxed);

                if wants_time_info && time_info_ptr != 0 {
                    let f: unsafe extern "win64" fn(
                        *mut Time,
                        i32,
                        crate::asio::Bool,
                    ) -> *mut Time = unsafe { core::mem::transmute(time_info_ptr) };

                    let mut time: Time = unsafe { core::mem::zeroed() };
                    unsafe {
                        let p = &mut time as *mut Time;
                        let ti = core::ptr::addr_of_mut!((*p).time_info);
                        core::ptr::addr_of_mut!((*ti).speed).write_unaligned(1.0);
                        core::ptr::addr_of_mut!((*ti).system_time).write_unaligned(now_timestamp());
                        core::ptr::addr_of_mut!((*ti).sample_position)
                            .write_unaligned(samples_from_u64(position_before));
                        core::ptr::addr_of_mut!((*ti).sample_rate).write_unaligned(sample_rate);
                        core::ptr::addr_of_mut!((*ti).flags).write_unaligned(
                            TimeInfoFlags::SYSTEM_TIME_VALID
                                | TimeInfoFlags::SAMPLE_POSITION_VALID
                                | TimeInfoFlags::SAMPLE_RATE_VALID,
                        );
                    }
                    unsafe { f(&mut time as *mut Time, idx as i32, Bool::TRUE) };
                } else {
                    let ptr = BUFFER_SWITCH_FN.load(Ordering::Relaxed);
                    if ptr != 0 {
                        let f: unsafe extern "win64" fn(i32, crate::asio::Bool) =
                            unsafe { core::mem::transmute(ptr) };
                        unsafe { f(idx as i32, crate::asio::Bool::TRUE) };
                    }
                }

                for ch in 0..channels_out {
                    let Some(dst) = outputs.get(ch) else {
                        continue;
                    };
                    if dst.ptr.is_null() {
                        continue;
                    }
                    let src = output_ptrs
                        .get(ch)
                        .map(|&[p0, p1]| if idx == 0 { p0 } else { p1 })
                        .unwrap_or(0) as *const f32;
                    let n = dst.len.min(buffer_size);
                    if src.is_null() {
                        unsafe { core::ptr::write_bytes(dst.ptr, 0, dst.len) };
                    } else {
                        unsafe { core::ptr::copy_nonoverlapping(src, dst.ptr, n) };
                    }
                }

                SAMPLE_POSITION.fetch_add(buffer_size as u64, Ordering::Relaxed);
                CURRENT_BUFFER_INDEX.fetch_xor(1, Ordering::Relaxed);
            },
        );

        let output_target = parse_target(
            &crate::SELECTED_SINK
                .read()
                .map(|g| g.clone())
                .unwrap_or_default(),
        );
        let input_target = parse_target(
            &crate::SELECTED_SOURCE
                .read()
                .map(|g| g.clone())
                .unwrap_or_default(),
        );

        let config = crate::backends::DuplexConfig {
            sample_rate: self.sample_rate,
            buffer_size: buffer_size as u32,
            input_channels: self.num_inputs as u32,
            output_channels: self.num_outputs as u32,
            output_target,
            input_target,
        };

        if let Some(backend) = crate::ACTIVE_BACKEND.get() {
            if let Ok(mut b) = backend.lock()
                && let Err(e) = b.start(config, process)
            {
                tracing::error!("backend start failed: {e}");
                RUNNING.store(false, Ordering::SeqCst);
                return Err(AsioError::HwMalfunction);
            }
        } else {
            tracing::warn!("no backend available");
            RUNNING.store(false, Ordering::SeqCst);
            return Err(AsioError::HwMalfunction);
        }

        self.running = true;
        crate::STREAM_RUNNING.store(true, Ordering::Relaxed);
        tracing::info!("started");
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    fn stop(&mut self) -> AsioResult<()> {
        tracing::info!("stop");
        RUNNING.store(false, Ordering::SeqCst);

        if let Some(backend) = crate::ACTIVE_BACKEND.get()
            && let Ok(mut b) = backend.lock()
        {
            let _ = b.stop();
        }

        self.running = false;
        crate::STREAM_RUNNING.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn get_channels(&self) -> AsioResult<(i32, i32)> {
        Ok((self.num_inputs, self.num_outputs))
    }

    fn get_latencies(&self) -> AsioResult<(i32, i32)> {
        let f = ASIO_BUFFER_FRAMES.load(Ordering::SeqCst);
        let f = if f > 0 {
            f
        } else {
            PREFERRED_BUFFER_SIZE.load(Ordering::Relaxed)
        };
        Ok((f, f))
    }

    fn get_buffer_size(&self) -> AsioResult<(i32, i32, i32, i32)> {
        Ok((
            self.buffer_size_min,
            self.buffer_size_max,
            PREFERRED_BUFFER_SIZE.load(Ordering::Relaxed),
            self.buffer_size_granularity,
        ))
    }

    fn can_sample_rate(&self, sample_rate: SampleRate) -> AsioResult<()> {
        if !sample_rate.is_finite() || !(8_000.0..=384_000.0).contains(&sample_rate) {
            return Err(AsioError::InvalidParameter);
        }
        Ok(())
    }

    fn get_sample_rate(&self) -> AsioResult<SampleRate> {
        Ok(self.sample_rate)
    }

    fn set_sample_rate(&mut self, sample_rate: SampleRate) -> AsioResult<()> {
        self.can_sample_rate(sample_rate)?;
        self.sample_rate = sample_rate;
        crate::DBG_SAMPLE_RATE.store(sample_rate as u32, Ordering::Relaxed);
        Ok(())
    }

    fn get_clock_sources(&self, clocks: &mut [ClockSource]) -> AsioResult<i32> {
        let Some(clock) = clocks.get_mut(0) else {
            return Ok(1);
        };

        let mut name = [0 as c_char; 32];
        write_cstr(&mut name, "Internal");
        *clock = ClockSource {
            index: 0,
            associated_channel: -1,
            associated_group: -1,
            is_current_source: Bool::TRUE,
            name,
        };

        Ok(1)
    }

    fn set_clock_source(&mut self, reference: i32) -> AsioResult<()> {
        if reference == 0 {
            Ok(())
        } else {
            Err(AsioError::InvalidParameter)
        }
    }

    fn get_sample_position(&self) -> AsioResult<(Samples, TimeStamp)> {
        fire_reset_if_pending();
        Ok((
            samples_from_u64(SAMPLE_POSITION.load(Ordering::Relaxed)),
            now_timestamp(),
        ))
    }

    fn get_channel_info(&self, info: &mut ChannelInfo) -> AsioResult<()> {
        let is_input = info.is_input.as_bool();
        let channel = info.channel;
        let channel_count = if is_input {
            self.num_inputs
        } else {
            self.num_outputs
        };

        if channel < 0 || channel >= channel_count {
            return Err(AsioError::InvalidParameter);
        }

        let mut name = [0 as c_char; 32];
        let prefix = if is_input { "Input" } else { "Output" };
        let number = channel + 1;
        write_cstr(&mut name, &format!("{prefix} {number}"));

        info.is_active = Bool::TRUE;
        info.channel_group = 0;
        info.sample_type = SampleType::Float32Lsb;
        info.name = name;

        Ok(())
    }

    fn create_buffers(
        &mut self,
        buffer_infos: &mut [BufferInfo],
        buffer_size: i32,
        callbacks: &mut Callbacks,
    ) -> AsioResult<()> {
        if let Some(f) = callbacks.asio_message {
            ASIO_MESSAGE_FN.store(f as usize, Ordering::Relaxed);
        }
        if let Some(f) = callbacks.buffer_switch {
            BUFFER_SWITCH_FN.store(f as usize, Ordering::Relaxed);
        }
        if let Some(f) = callbacks.buffer_switch_time_info {
            BUFFER_SWITCH_TIME_INFO_FN.store(f as usize, Ordering::Relaxed);
        } else {
            BUFFER_SWITCH_TIME_INFO_FN.store(0, Ordering::Relaxed);
        }

        if !self.initialized {
            return Err(AsioError::InvalidMode);
        }

        if !(self.buffer_size_min..=self.buffer_size_max).contains(&buffer_size) {
            return Err(AsioError::InvalidParameter);
        }

        ASIO_BUFFER_FRAMES.store(buffer_size, Ordering::SeqCst);

        let wants_time_info = if let Some(f) = callbacks.asio_message {
            let has_time_info_cb = matches!(callbacks.buffer_switch_time_info, Some(_));
            let ret = unsafe {
                f(
                    MessageSelector::SupportsTimeInfo as i32,
                    0,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                )
            };
            ret != 0 && has_time_info_cb
        } else {
            false
        };
        HOST_WANTS_TIME_INFO.store(wants_time_info, Ordering::Relaxed);

        self.buffers.clear();
        crate::DBG_ASIO_BUFFER_SIZE.store(buffer_size as u32, Ordering::Relaxed);
        crate::DBG_NUM_INPUTS.store(self.num_inputs as u32, Ordering::Relaxed);
        crate::DBG_NUM_OUTPUTS.store(self.num_outputs as u32, Ordering::Relaxed);
        let channel_count = buffer_infos.len();
        let buffer_bytes = buffer_size as usize * SAMPLE_BYTES;

        for info in &mut *buffer_infos {
            let is_input = info.is_input.as_bool();
            let channel_num = info.channel_num;
            let available_channels = if is_input {
                self.num_inputs
            } else {
                self.num_outputs
            };

            if channel_num < 0 || channel_num >= available_channels {
                self.buffers.clear();
                return Err(AsioError::InvalidParameter);
            }

            let mut buffers = [core::ptr::null_mut(); 2];
            for buffer in &mut buffers {
                let mut storage = vec![0u8; buffer_bytes].into_boxed_slice();
                *buffer = storage.as_mut_ptr().cast();
                self.buffers.push(storage);
            }
            info.buffers = buffers;
        }

        if let Ok(mut ptrs) = ASIO_OUTPUT_PTRS.lock() {
            ptrs.clear();
            for info in buffer_infos.iter() {
                if !info.is_input.as_bool() {
                    ptrs.push([info.buffers[0] as usize, info.buffers[1] as usize]);
                }
            }
        }

        if let Ok(mut ptrs) = ASIO_INPUT_PTRS.lock() {
            ptrs.clear();
            for info in buffer_infos.iter() {
                if info.is_input.as_bool() {
                    ptrs.push([info.buffers[0] as usize, info.buffers[1] as usize]);
                }
            }
        }

        tracing::info!(
            "create_buffers channels={} size={} buffers={}",
            channel_count,
            buffer_size,
            self.buffers.len()
        );

        Ok(())
    }

    fn dispose_buffers(&mut self) -> AsioResult<()> {
        tracing::info!("dispose_buffers");
        self.buffers.clear();
        ASIO_BUFFER_FRAMES.store(0, Ordering::SeqCst);
        BUFFER_SWITCH_FN.store(0, Ordering::Relaxed);
        BUFFER_SWITCH_TIME_INFO_FN.store(0, Ordering::Relaxed);
        if let Ok(mut ptrs) = ASIO_INPUT_PTRS.lock() {
            ptrs.clear();
        }
        if let Ok(mut ptrs) = ASIO_OUTPUT_PTRS.lock() {
            ptrs.clear();
        }
        Ok(())
    }

    fn control_panel(&mut self) -> AsioResult<()> {
        crate::gui::show_control_panel().map_err(|err| {
            tracing::warn!("control_panel failed: {err:?}");
            AsioError::HwMalfunction
        })
    }

    fn future(&mut self, selector: i32, opt: usize) -> AsioResult<()> {
        match selector {
            x if x == FutureSelector::CanDoIoFormat as i32 => {
                if opt == 0 {
                    return Err(AsioError::InvalidParameter);
                }

                let format = unsafe { &*(opt as *const IoFormat) };
                if format.format_type == IoFormatType::Pcm {
                    Ok(())
                } else {
                    Err(AsioError::InvalidParameter)
                }
            }
            x if x == FutureSelector::GetIoFormat as i32 => {
                if opt == 0 {
                    return Err(AsioError::InvalidParameter);
                }

                let format = unsafe { &mut *(opt as *mut IoFormat) };
                format.format_type = IoFormatType::Pcm;
                Ok(())
            }
            x if x == FutureSelector::SetIoFormat as i32 => {
                if opt == 0 {
                    return Err(AsioError::InvalidParameter);
                }

                let format = unsafe { &*(opt as *const IoFormat) };
                if format.format_type == IoFormatType::Pcm {
                    Ok(())
                } else {
                    Err(AsioError::InvalidParameter)
                }
            }
            x if x == FutureSelector::CanTimeInfo as i32 => Ok(()),
            _ => Err(AsioError::NotPresent),
        }
    }

    fn output_ready(&mut self) -> AsioResult<()> {
        fire_reset_if_pending();
        Ok(())
    }
}

crate::export_asio_driver!(RWAsioDriver);
