use core::ffi::{c_char, c_void};

pub type SampleRate = f64;

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bool {
    False = 0,
    True = 1,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleType {
    Int16Msb = 0,
    Int24Msb = 1,
    Int32Msb = 2,
    Float32Msb = 3,
    Float64Msb = 4,
    Int32Msb16 = 8,
    Int32Msb18 = 9,
    Int32Msb20 = 10,
    Int32Msb24 = 11,
    Int16Lsb = 16,
    Int24Lsb = 17,
    Int32Lsb = 18,
    Float32Lsb = 19,
    Float64Lsb = 20,
    Int32Lsb16 = 24,
    Int32Lsb18 = 25,
    Int32Lsb20 = 26,
    Int32Lsb24 = 27,
    DsdInt8Lsb1 = 32,
    DsdInt8Msb1 = 33,
    DsdInt8Ner8 = 40,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Ok = 0,
    Success = 0x3f4847a0,
    NotPresent = -1000,
    HwMalfunction = -999,
    InvalidParameter = -998,
    InvalidMode = -997,
    SpNotAdvancing = -996,
    NoClock = -995,
    NoMemory = -994,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsioError {
    NotPresent = -1000,
    HwMalfunction = -999,
    InvalidParameter = -998,
    InvalidMode = -997,
    SpNotAdvancing = -996,
    NoClock = -995,
    NoMemory = -994,
}

pub type AsioResult = Result<(), AsioError>;

impl From<AsioError> for Error {
    fn from(e: AsioError) -> Error {
        match e {
            AsioError::NotPresent => Error::NotPresent,
            AsioError::HwMalfunction => Error::HwMalfunction,
            AsioError::InvalidParameter => Error::InvalidParameter,
            AsioError::InvalidMode => Error::InvalidMode,
            AsioError::SpNotAdvancing => Error::SpNotAdvancing,
            AsioError::NoClock => Error::NoClock,
            AsioError::NoMemory => Error::NoMemory,
        }
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoFormatType {
    Invalid = -1,
    Pcm = 0,
    Dsd = 1,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageSelector {
    SelectorSupported = 1,
    EngineVersion = 2,
    ResetRequest = 3,
    BufferSizeChange = 4,
    ResyncRequest = 5,
    LatenciesChanged = 6,
    SupportsTimeInfo = 7,
    SupportsTimeCode = 8,
    MmcCommand = 9,
    SupportsInputMonitor = 10,
    SupportsInputGain = 11,
    SupportsInputMeter = 12,
    SupportsOutputGain = 13,
    SupportsOutputMeter = 14,
    Overload = 15,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutureSelector {
    EnableTimeCodeRead = 1,
    DisableTimeCodeRead = 2,
    SetInputMonitor = 3,
    Transport = 4,
    SetInputGain = 5,
    GetInputMeter = 6,
    SetOutputGain = 7,
    GetOutputMeter = 8,
    CanInputMonitor = 9,
    CanTimeInfo = 10,
    CanTimeCode = 11,
    CanTransport = 12,
    CanInputGain = 13,
    CanInputMeter = 14,
    CanOutputGain = 15,
    CanOutputMeter = 16,
    OptionalOne = 17,
    SetIoFormat = 0x23111961,
    GetIoFormat = 0x23111983,
    CanDoIoFormat = 0x23112004,
    CanReportOverload = 0x24042012,
    GetInternalBufferSamples = 0x25042012,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportCommand {
    Start = 1,
    Stop = 2,
    Locate = 3,
    PunchIn = 4,
    PunchOut = 5,
    ArmOn = 6,
    ArmOff = 7,
    MonitorOn = 8,
    MonitorOff = 9,
    Arm = 10,
    Monitor = 11,
}

bitflags::bitflags! {
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TimeCodeFlags: u32 {
        const VALID = 1;
        const RUNNING = 1 << 1;
        const REVERSE = 1 << 2;
        const ONSPEED = 1 << 3;
        const STILL = 1 << 4;
        const SPEED_VALID = 1 << 8;
    }

    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TimeInfoFlags: u32 {
        const SYSTEM_TIME_VALID = 1;
        const SAMPLE_POSITION_VALID = 1 << 1;
        const SAMPLE_RATE_VALID = 1 << 2;
        const SPEED_VALID = 1 << 3;
        const SAMPLE_RATE_CHANGED = 1 << 4;
        const CLOCK_SOURCE_CHANGED = 1 << 5;
    }
}

#[repr(C, packed(4))]
pub struct Samples {
    pub hi: u32,
    pub lo: u32,
}

#[repr(C, packed(4))]
pub struct TimeStamp {
    pub hi: u32,
    pub lo: u32,
}

#[repr(C, packed(4))]
pub struct TimeCode {
    pub speed: f64,
    pub time_code_samples: Samples,
    pub flags: TimeCodeFlags,
    pub future: [c_char; 64],
}

#[repr(C, packed(4))]
pub struct TimeInfo {
    pub speed: f64,
    pub system_time: TimeStamp,
    pub sample_position: Samples,
    pub sample_rate: SampleRate,
    pub flags: TimeInfoFlags,
    pub reserved: [c_char; 12],
}

#[repr(C, packed(4))]
pub struct Time {
    pub reserved: [i32; 4],
    pub time_info: TimeInfo,
    pub time_code: TimeCode,
}

#[repr(C, packed(4))]
pub struct Callbacks {
    pub buffer_switch:
        Option<unsafe extern "system" fn(double_buffer_index: i32, direct_process: Bool)>,
    pub sample_rate_did_change: Option<unsafe extern "system" fn(s_rate: SampleRate)>,
    pub asio_message: Option<
        unsafe extern "system" fn(
            selector: i32,
            value: i32,
            message: *mut c_void,
            opt: *mut f64,
        ) -> i32,
    >,
    pub buffer_switch_time_info: Option<
        unsafe extern "system" fn(
            params: *mut Time,
            double_buffer_index: i32,
            direct_process: Bool,
        ) -> *mut Time,
    >,
}

#[repr(C, packed(4))]
pub struct DriverInfo {
    pub asio_version: i32,
    pub driver_version: i32,
    pub name: [c_char; 32],
    pub error_message: [c_char; 124],
    pub sys_ref: *mut c_void,
}

#[repr(C, packed(4))]
pub struct ClockSource {
    pub index: i32,
    pub associated_channel: i32,
    pub associated_group: i32,
    pub is_current_source: Bool,
    pub name: [c_char; 32],
}

#[repr(C, packed(4))]
pub struct ChannelInfo {
    pub channel: i32,
    pub is_input: Bool,
    pub is_active: Bool,
    pub channel_group: i32,
    pub sample_type: SampleType,
    pub name: [c_char; 32],
}

#[repr(C, packed(4))]
pub struct BufferInfo {
    pub is_input: Bool,
    pub channel_num: i32,
    pub buffers: [*mut c_void; 2],
}

#[repr(C, packed(4))]
pub struct InputMonitor {
    pub input: i32,
    pub output: i32,
    pub gain: i32,
    pub state: Bool,
    pub pan: i32,
}

#[repr(C, packed(4))]
pub struct ChannelControls {
    pub channel: i32,
    pub is_input: Bool,
    pub gain: i32,
    pub meter: i32,
    pub future: [c_char; 32],
}

#[repr(C, packed(4))]
pub struct TransportParameters {
    pub command: i32,
    pub sample_position: Samples,
    pub track: i32,
    pub track_switches: [i32; 16],
    pub future: [c_char; 64],
}

#[repr(C, packed(4))]
pub struct IoFormat {
    pub format_type: IoFormatType,
    pub future: [c_char; 508],
}

#[repr(C, packed(4))]
pub struct InternalBufferInfo {
    pub input_samples: i32,
    pub output_samples: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl Guid {
    pub const fn parse(s: &str) -> Guid {
        let b = s.as_bytes();
        let o = if b.len() == 38 && b[0] == b'{' { 1 } else { 0 };
        assert!(
            b.len() == 36 + o * 2,
            "GUID must be 36 chars, optionally brace-wrapped"
        );
        assert!(
            b[o + 8] == b'-' && b[o + 13] == b'-' && b[o + 18] == b'-' && b[o + 23] == b'-',
            "GUID dashes misplaced"
        );
        Guid {
            data1: ((Self::hb(b, o) as u32) << 24)
                | ((Self::hb(b, o + 2) as u32) << 16)
                | ((Self::hb(b, o + 4) as u32) << 8)
                | (Self::hb(b, o + 6) as u32),
            data2: ((Self::hb(b, o + 9) as u16) << 8) | (Self::hb(b, o + 11) as u16),
            data3: ((Self::hb(b, o + 14) as u16) << 8) | (Self::hb(b, o + 16) as u16),
            data4: [
                Self::hb(b, o + 19),
                Self::hb(b, o + 21),
                Self::hb(b, o + 24),
                Self::hb(b, o + 26),
                Self::hb(b, o + 28),
                Self::hb(b, o + 30),
                Self::hb(b, o + 32),
                Self::hb(b, o + 34),
            ],
        }
    }

    const fn hb(b: &[u8], i: usize) -> u8 {
        (Self::hv(b[i]) << 4) | Self::hv(b[i + 1])
    }

    const fn hv(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("invalid hex digit in GUID"),
        }
    }
}

#[macro_export]
macro_rules! guid {
    ($s:literal) => {
        const { $crate::asio::Guid::parse($s) }
    };
}

pub type Refiid = *const Guid;

#[repr(C)]
pub struct IUnknownVtbl {
    pub query_interface: unsafe extern "system" fn(
        this: *mut IUnknown,
        riid: Refiid,
        ppv_object: *mut *mut c_void,
    ) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut IUnknown) -> u32,
    pub release: unsafe extern "system" fn(this: *mut IUnknown) -> u32,
}

#[repr(C)]
pub struct IUnknown {
    pub lp_vtbl: *const IUnknownVtbl,
}

#[repr(C)]
pub struct IAsioVtbl {
    pub base: IUnknownVtbl,
    pub init: unsafe extern "system" fn(this: *mut IAsio, sys_handle: *mut c_void) -> Bool,
    pub get_driver_name: unsafe extern "system" fn(this: *mut IAsio, name: *mut c_char),
    pub get_driver_version: unsafe extern "system" fn(this: *mut IAsio) -> i32,
    pub get_error_message: unsafe extern "system" fn(this: *mut IAsio, string: *mut c_char),
    pub start: unsafe extern "system" fn(this: *mut IAsio) -> Error,
    pub stop: unsafe extern "system" fn(this: *mut IAsio) -> Error,
    pub get_channels: unsafe extern "system" fn(
        this: *mut IAsio,
        num_input_channels: *mut i32,
        num_output_channels: *mut i32,
    ) -> Error,
    pub get_latencies: unsafe extern "system" fn(
        this: *mut IAsio,
        input_latency: *mut i32,
        output_latency: *mut i32,
    ) -> Error,
    pub get_buffer_size: unsafe extern "system" fn(
        this: *mut IAsio,
        min_size: *mut i32,
        max_size: *mut i32,
        preferred_size: *mut i32,
        granularity: *mut i32,
    ) -> Error,
    pub can_sample_rate:
        unsafe extern "system" fn(this: *mut IAsio, sample_rate: SampleRate) -> Error,
    pub get_sample_rate:
        unsafe extern "system" fn(this: *mut IAsio, sample_rate: *mut SampleRate) -> Error,
    pub set_sample_rate:
        unsafe extern "system" fn(this: *mut IAsio, sample_rate: SampleRate) -> Error,
    pub get_clock_sources: unsafe extern "system" fn(
        this: *mut IAsio,
        clocks: *mut ClockSource,
        num_sources: *mut i32,
    ) -> Error,
    pub set_clock_source: unsafe extern "system" fn(this: *mut IAsio, reference: i32) -> Error,
    pub get_sample_position: unsafe extern "system" fn(
        this: *mut IAsio,
        s_pos: *mut Samples,
        t_stamp: *mut TimeStamp,
    ) -> Error,
    pub get_channel_info:
        unsafe extern "system" fn(this: *mut IAsio, info: *mut ChannelInfo) -> Error,
    pub create_buffers: unsafe extern "system" fn(
        this: *mut IAsio,
        buffer_infos: *mut BufferInfo,
        num_channels: i32,
        buffer_size: i32,
        callbacks: *mut Callbacks,
    ) -> Error,
    pub dispose_buffers: unsafe extern "system" fn(this: *mut IAsio) -> Error,
    pub control_panel: unsafe extern "system" fn(this: *mut IAsio) -> Error,
    pub future:
        unsafe extern "system" fn(this: *mut IAsio, selector: i32, opt: *mut c_void) -> Error,
    pub output_ready: unsafe extern "system" fn(this: *mut IAsio) -> Error,
}

#[repr(C)]
pub struct IAsio {
    pub lp_vtbl: *const IAsioVtbl,
}

pub trait Asio {
    fn init(&mut self, sys_handle: *mut c_void) -> Bool;
    fn get_driver_name(&mut self, name: *mut c_char);
    fn get_driver_version(&mut self) -> i32;
    fn get_error_message(&mut self, string: *mut c_char);
    fn start(&mut self) -> AsioResult;
    fn stop(&mut self) -> AsioResult;
    fn get_channels(
        &mut self,
        num_input_channels: *mut i32,
        num_output_channels: *mut i32,
    ) -> AsioResult;
    fn get_latencies(&mut self, input_latency: *mut i32, output_latency: *mut i32) -> AsioResult;
    fn get_buffer_size(
        &mut self,
        min_size: *mut i32,
        max_size: *mut i32,
        preferred_size: *mut i32,
        granularity: *mut i32,
    ) -> AsioResult;
    fn can_sample_rate(&mut self, sample_rate: SampleRate) -> AsioResult;
    fn get_sample_rate(&mut self, sample_rate: *mut SampleRate) -> AsioResult;
    fn set_sample_rate(&mut self, sample_rate: SampleRate) -> AsioResult;
    fn get_clock_sources(&mut self, clocks: *mut ClockSource, num_sources: *mut i32) -> AsioResult;
    fn set_clock_source(&mut self, reference: i32) -> AsioResult;
    fn get_sample_position(&mut self, s_pos: *mut Samples, t_stamp: *mut TimeStamp) -> AsioResult;
    fn get_channel_info(&mut self, info: *mut ChannelInfo) -> AsioResult;
    fn create_buffers(
        &mut self,
        buffer_infos: *mut BufferInfo,
        num_channels: i32,
        buffer_size: i32,
        callbacks: *mut Callbacks,
    ) -> AsioResult;
    fn dispose_buffers(&mut self) -> AsioResult;
    fn control_panel(&mut self) -> AsioResult;
    fn future(&mut self, selector: i32, opt: *mut c_void) -> AsioResult;
    fn output_ready(&mut self) -> AsioResult;
}
