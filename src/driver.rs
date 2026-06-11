use crate::asio::*;
use crate::com::AsioClass;
use core::ffi::c_char;

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

const DRIVER_VERSION: i32 = 1;
const DEFAULT_INPUTS: i32 = 0;
const DEFAULT_OUTPUTS: i32 = 2;
const DEFAULT_SAMPLE_RATE: SampleRate = 44_100.0;
const BUFFER_SIZE_MIN: i32 = 64;
const BUFFER_SIZE_MAX: i32 = 2_048;
const BUFFER_SIZE_PREFERRED: i32 = 512;
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

        crate::rlog!("[driver] start");
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> AsioResult<()> {
        crate::rlog!("[driver] stop");
        self.running = false;
        Ok(())
    }

    fn get_channels(&self) -> AsioResult<(i32, i32)> {
        Ok((self.num_inputs, self.num_outputs))
    }

    fn get_latencies(&self) -> AsioResult<(i32, i32)> {
        Ok((0, self.buffer_size_preferred))
    }

    fn get_buffer_size(&self) -> AsioResult<(i32, i32, i32, i32)> {
        Ok((
            self.buffer_size_min,
            self.buffer_size_max,
            self.buffer_size_preferred,
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
        _callbacks: &mut Callbacks,
    ) -> AsioResult<()> {
        if !self.initialized {
            return Err(AsioError::InvalidMode);
        }

        if !(self.buffer_size_min..=self.buffer_size_max).contains(&buffer_size) {
            return Err(AsioError::InvalidParameter);
        }

        self.buffers.clear();
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
        Ok(())
    }
}

crate::export_asio_driver!(RWAsioDriver);
