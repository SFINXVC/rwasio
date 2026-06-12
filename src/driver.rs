use crate::backends::AudioBackend;
use crate::backends::pipewire::PipeWireAudioBackend;
use crate::com::AsioClass;
use crate::{DEVICE_LIST, asio::*};
use core::ffi::c_char;
use libc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

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
    sample_position: u64,
    buffers: Vec<Box<[u8]>>,
}

static PREFERRED_BUFFER_SIZE: AtomicI32 = AtomicI32::new(BUFFER_SIZE_PREFERRED);
static ASIO_MESSAGE_FN: AtomicUsize = AtomicUsize::new(0);
static RESET_PENDING: AtomicBool = AtomicBool::new(false);
static RESET_WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static BUFFER_SWITCH_FN: AtomicUsize = AtomicUsize::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
static CURRENT_BUFFER_INDEX: AtomicI32 = AtomicI32::new(0);
static SEM_PW_TO_WINE: AtomicUsize = AtomicUsize::new(0);
static SEM_WINE_TO_PW: AtomicUsize = AtomicUsize::new(0);
static ASIO_OUTPUT_PTRS: std::sync::Mutex<Vec<[usize; 2]>> = std::sync::Mutex::new(Vec::new());
static ASIO_INPUT_PTRS: std::sync::Mutex<Vec<[usize; 2]>> = std::sync::Mutex::new(Vec::new());
static CAPTURE_RING: std::sync::Mutex<std::collections::VecDeque<f32>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

pub fn set_output_target(node_id: &str) {
    let id: Option<u32> = if node_id.is_empty() {
        None
    } else {
        node_id.parse().ok()
    };
    if let Ok(s) = crate::PW_STREAM_SENDER.read()
        && let Some(sender) = s.as_ref()
    {
        let _ = sender.send(crate::PwStreamCmd::SetTarget(id));
        crate::rlog!("[driver] set_output_target id={:?}", id);
    }
}

pub fn set_input_target(node_id: &str) {
    let id: Option<u32> = if node_id.is_empty() {
        None
    } else {
        node_id.parse().ok()
    };
    if let Ok(s) = crate::PW_INPUT_STREAM_SENDER.read()
        && let Some(sender) = s.as_ref()
    {
        let _ = sender.send(crate::PwStreamCmd::SetTarget(id));
        crate::rlog!("[driver] set_input_target id={:?}", id);
    }
}

pub fn set_preferred_buffer_size(size: i32) {
    PREFERRED_BUFFER_SIZE.store(size, Ordering::Relaxed);
    RESET_PENDING.store(true, Ordering::Relaxed);
    crate::rlog!("[driver] preferred buffer size -> {}, reset pending", size);
}

unsafe extern "win64" fn reset_worker(_: *mut core::ffi::c_void) -> u32 {
    loop {
        unsafe {
            Sleep(50);
        }
        fire_reset_if_pending();
    }
}

unsafe extern "win64" fn audio_worker(_: *mut core::ffi::c_void) -> u32 {
    loop {
        let s1 = SEM_PW_TO_WINE.load(Ordering::SeqCst) as *mut libc::sem_t;
        if s1.is_null() {
            break;
        }
        unsafe { libc::sem_wait(s1) };

        if !RUNNING.load(Ordering::Relaxed) {
            break;
        }

        let idx = CURRENT_BUFFER_INDEX.load(Ordering::Relaxed);
        let ptr = BUFFER_SWITCH_FN.load(Ordering::Relaxed);
        if ptr != 0 {
            let f: unsafe extern "win64" fn(i32, crate::asio::Bool) =
                unsafe { core::mem::transmute(ptr) };
            unsafe { f(idx, crate::asio::Bool::True) };
        }

        let s2 = SEM_WINE_TO_PW.load(Ordering::SeqCst) as *mut libc::sem_t;
        if !s2.is_null() {
            unsafe { libc::sem_post(s2) };
        }
    }
    0
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
            crate::rlog!("[driver] sent ResetRequest to host");
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
            sample_position: 0,
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

fn zero_timestamp() -> TimeStamp {
    TimeStamp { hi: 0, lo: 0 }
}

impl AsioClass for RWAsioDriver {
    const CLSID: Guid = crate::guid!("019eb112-f780-734f-ab6b-5b2ab7e81380");
    const NAME: &'static str = "Rusty Wine ASIO";
    const DESCRIPTION: &'static str = "Rusty Wine ASIO Driver";
    const DLL_FILE: &'static str = "rwasio.dll";

    fn new() -> Self {
        crate::rlog!("[driver] new");

        Self::default()
    }
}

impl Asio for RWAsioDriver {
    fn init(&mut self, _sys_handle: usize) -> Bool {
        crate::rlog!("[driver] init");
        self.initialized = true;

        if !RESET_WORKER_STARTED.swap(true, Ordering::Relaxed) {
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
            crate::rlog!("[driver] reset worker thread started");
        }

        if DEVICE_LIST.get().is_none() {
            match PipeWireAudioBackend::new() {
                Ok(v) => {
                    let sinks = v.sinks().into_iter().map(|d| (d.name, d.id)).collect();
                    let sources = v.sources().into_iter().map(|d| (d.name, d.id)).collect();
                    DEVICE_LIST.get_or_init(|| (sinks, sources));
                    crate::ACTIVE_BACKEND.get_or_init(|| std::sync::Mutex::new(Box::new(v)));
                    crate::rlog!("[driver] device list populated");
                }
                Err(e) => {
                    crate::rlog!("[driver] backend init failed: {}", e);
                    DEVICE_LIST.get_or_init(|| (vec![], vec![]));
                }
            }
        }

        Bool::True
    }

    fn get_driver_name(&self) -> &str {
        Self::NAME
    }

    fn get_driver_version(&self) -> i32 {
        DRIVER_VERSION
    }

    fn get_error_message(&self) -> &str {
        ""
    }

    fn start(&mut self) -> AsioResult<()> {
        if !self.initialized {
            return Err(AsioError::InvalidMode);
        }
        if self.running {
            return Ok(());
        }

        crate::rlog!("[driver] start");

        unsafe {
            let s1 = Box::into_raw(Box::new(core::mem::zeroed::<libc::sem_t>()));
            libc::sem_init(s1, 0, 0);
            SEM_PW_TO_WINE.store(s1 as usize, Ordering::SeqCst);

            let s2 = Box::into_raw(Box::new(core::mem::zeroed::<libc::sem_t>()));
            libc::sem_init(s2, 0, 0);
            SEM_WINE_TO_PW.store(s2 as usize, Ordering::SeqCst);
        }

        RUNNING.store(true, Ordering::SeqCst);

        unsafe {
            CreateThread(
                core::ptr::null(),
                0,
                Some(audio_worker),
                core::ptr::null_mut(),
                0,
                core::ptr::null_mut(),
            );
        }

        let channels_out = self.num_outputs as usize;
        let channels_in = self.num_inputs as usize;
        let buffer_size = PREFERRED_BUFFER_SIZE.load(Ordering::Relaxed) as usize;

        if let Ok(mut ring) = CAPTURE_RING.lock() {
            ring.clear();
            ring.extend(std::iter::repeat_n(
                0.0f32,
                buffer_size * channels_in.max(1),
            ));
        }

        let input_process = Box::new(move |captured: &[f32]| {
            let n_frames = captured.len() / channels_in.max(1);
            let n_samples = n_frames * channels_in;
            crate::DBG_CAPTURE_CALLBACKS.fetch_add(1, Ordering::Relaxed);
            crate::DBG_CAPTURE_FRAMES.store(n_frames as u32, Ordering::Relaxed);

            if let Ok(mut ring) = CAPTURE_RING.lock() {
                let max = buffer_size * channels_in.max(1) * 8;
                if ring.len() + n_samples > max {
                    let excess = ring.len() + n_samples - max;
                    ring.drain(..excess);
                }
                ring.extend(captured[..n_samples].iter().copied());
                crate::DBG_STAGING_SAMPLES.store(ring.len() as u32, Ordering::Relaxed);
            }
        });

        // Snapshot raw input buffer pointers. create_buffers always runs before start(),
        // and dispose_buffers only runs after stop() which joins the PW thread first,
        // so these pointers are valid for the entire lifetime of the process closure.
        let input_ptrs: Vec<[usize; 2]> = ASIO_INPUT_PTRS
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        let channels = channels_out;
        let process = Box::new(move |output: &mut [f32]| {
            let s1 = SEM_PW_TO_WINE.load(Ordering::SeqCst) as *mut libc::sem_t;
            let s2 = SEM_WINE_TO_PW.load(Ordering::SeqCst) as *mut libc::sem_t;
            if s1.is_null() || s2.is_null() {
                return;
            }

            if !input_ptrs.is_empty() {
                let write_idx = CURRENT_BUFFER_INDEX.load(Ordering::Relaxed) as usize ^ 1;
                if let Ok(mut ring) = CAPTURE_RING.lock() {
                    for frame in 0..buffer_size {
                        for ch in 0..channels_in {
                            let sample = ring.pop_front().unwrap_or(0.0);
                            if let Some(&[p0, p1]) = input_ptrs.get(ch) {
                                let ptr = if write_idx == 0 { p0 } else { p1 } as *mut f32;
                                if !ptr.is_null() {
                                    unsafe { *ptr.add(frame) = sample };
                                }
                            }
                        }
                    }
                }
            }

            unsafe { libc::sem_post(s1) };
            unsafe { libc::sem_wait(s2) };

            if !RUNNING.load(Ordering::Relaxed) {
                return;
            }

            let idx = CURRENT_BUFFER_INDEX.load(Ordering::Relaxed) as usize;
            let n_frames = (output.len() / channels.max(1)).min(buffer_size);
            crate::DBG_OUTPUT_CALLBACKS.fetch_add(1, Ordering::Relaxed);
            crate::DBG_OUTPUT_FRAMES.store(n_frames as u32, Ordering::Relaxed);
            crate::DBG_CURRENT_BUFFER_IDX.store(idx as u32, Ordering::Relaxed);

            if let Ok(ptrs) = ASIO_OUTPUT_PTRS.lock() {
                for frame in 0..n_frames {
                    for ch in 0..channels {
                        let sample = ptrs.get(ch).map_or(0.0, |&[p0, p1]| {
                            let ptr = if idx == 0 { p0 } else { p1 } as *const f32;
                            if ptr.is_null() {
                                0.0
                            } else {
                                unsafe { *ptr.add(frame) }
                            }
                        });
                        if let Some(s) = output.get_mut(frame * channels + ch) {
                            *s = sample;
                        }
                    }
                }
            }

            CURRENT_BUFFER_INDEX.fetch_xor(1, Ordering::Relaxed);
        });

        let sink_id = crate::SELECTED_SINK
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();

        let source_id = crate::SELECTED_SOURCE
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();

        let output_config = crate::backends::StreamConfig {
            sample_rate: self.sample_rate,
            buffer_size: PREFERRED_BUFFER_SIZE.load(Ordering::Relaxed) as u32,
            channels: self.num_outputs as u32,
        };

        let input_config = crate::backends::StreamConfig {
            sample_rate: self.sample_rate,
            buffer_size: PREFERRED_BUFFER_SIZE.load(Ordering::Relaxed) as u32,
            channels: self.num_inputs as u32,
        };

        if let Some(backend) = crate::ACTIVE_BACKEND.get() {
            if let Ok(mut b) = backend.lock() {
                if let Err(e) = b.start_output(&sink_id, output_config, process) {
                    crate::rlog!("[driver] start_output failed: {e}");
                    return Err(AsioError::HwMalfunction);
                }
                if let Err(e) = b.start_input(&source_id, input_config, input_process) {
                    crate::rlog!("[driver] start_input failed: {e}");
                }
            }
        } else {
            crate::rlog!("[driver] no backend available");
        }

        self.running = true;
        crate::rlog!("[driver] started, sink={:?}", sink_id);
        Ok(())
    }

    fn stop(&mut self) -> AsioResult<()> {
        crate::rlog!("[driver] stop");
        RUNNING.store(false, Ordering::SeqCst);

        let s1 = SEM_PW_TO_WINE.load(Ordering::SeqCst) as *mut libc::sem_t;
        if !s1.is_null() {
            unsafe { libc::sem_post(s1) };
        }
        let s2 = SEM_WINE_TO_PW.load(Ordering::SeqCst) as *mut libc::sem_t;
        if !s2.is_null() {
            unsafe { libc::sem_post(s2) };
        }

        if let Some(backend) = crate::ACTIVE_BACKEND.get()
            && let Ok(mut b) = backend.lock()
        {
            let _ = b.stop_output();
            let _ = b.stop_input();
        }

        self.running = false;
        Ok(())
    }

    fn get_channels(&self) -> AsioResult<(i32, i32)> {
        Ok((self.num_inputs, self.num_outputs))
    }

    fn get_latencies(&self) -> AsioResult<(i32, i32)> {
        let buf = PREFERRED_BUFFER_SIZE.load(Ordering::Relaxed);
        Ok((buf, buf))
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
        if sample_rate == DEFAULT_SAMPLE_RATE {
            Ok(())
        } else {
            Err(AsioError::NoClock)
        }
    }

    fn get_sample_rate(&self) -> AsioResult<SampleRate> {
        Ok(self.sample_rate)
    }

    fn set_sample_rate(&mut self, sample_rate: SampleRate) -> AsioResult<()> {
        self.can_sample_rate(sample_rate)?;
        self.sample_rate = sample_rate;
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
            is_current_source: Bool::True,
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
        Ok((samples_from_u64(self.sample_position), zero_timestamp()))
    }

    fn get_channel_info(&self, info: &mut ChannelInfo) -> AsioResult<()> {
        let is_input = info.is_input == Bool::True;
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

        info.is_active = Bool::True;
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

        if !self.initialized {
            return Err(AsioError::InvalidMode);
        }

        if !(self.buffer_size_min..=self.buffer_size_max).contains(&buffer_size) {
            return Err(AsioError::InvalidParameter);
        }

        self.buffers.clear();
        crate::DBG_ASIO_BUFFER_SIZE.store(buffer_size as u32, Ordering::Relaxed);
        crate::DBG_NUM_INPUTS.store(self.num_inputs as u32, Ordering::Relaxed);
        crate::DBG_NUM_OUTPUTS.store(self.num_outputs as u32, Ordering::Relaxed);
        let channel_count = buffer_infos.len();
        let buffer_bytes = buffer_size as usize * SAMPLE_BYTES;

        for info in &mut *buffer_infos {
            let is_input = info.is_input == Bool::True;
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
                if info.is_input == Bool::False {
                    ptrs.push([info.buffers[0] as usize, info.buffers[1] as usize]);
                }
            }
        }

        if let Ok(mut ptrs) = ASIO_INPUT_PTRS.lock() {
            ptrs.clear();
            for info in buffer_infos.iter() {
                if info.is_input == Bool::True {
                    ptrs.push([info.buffers[0] as usize, info.buffers[1] as usize]);
                }
            }
        }

        crate::rlog!(
            "[driver] create_buffers channels={} size={} buffers={}",
            channel_count,
            buffer_size,
            self.buffers.len()
        );

        Ok(())
    }

    fn dispose_buffers(&mut self) -> AsioResult<()> {
        crate::rlog!("[driver] dispose_buffers");
        self.buffers.clear();
        Ok(())
    }

    fn control_panel(&mut self) -> AsioResult<()> {
        crate::gui::show_control_panel().map_err(|err| {
            crate::rlog!("[driver] control_panel failed: {err:?}");
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
