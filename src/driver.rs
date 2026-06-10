use crate::{asio::Asio, com::AsioClass};

#[derive(Default)]
pub struct RWAsioDriver;

impl AsioClass for RWAsioDriver {
    const CLSID: crate::asio::Guid = crate::guid!("019eb112-f780-734f-ab6b-5b2ab7e81380");

    const NAME: &'static str = "Rusty Wine ASIO";

    const DESCRIPTION: &'static str = "Rusty Wine ASIO Driver";

    fn new() -> Self {
        Self
    }
}

impl Asio for RWAsioDriver {
    fn init(&mut self, _sys_handle: *mut std::ffi::c_void) -> crate::asio::Bool {
        todo!()
    }

    fn get_driver_name(&mut self, _name: *mut std::ffi::c_char) {
        todo!()
    }

    fn get_driver_version(&mut self) -> i32 {
        todo!()
    }

    fn get_error_message(&mut self, _string: *mut std::ffi::c_char) {
        todo!()
    }

    fn start(&mut self) -> crate::asio::AsioResult {
        todo!()
    }

    fn stop(&mut self) -> crate::asio::AsioResult {
        todo!()
    }

    fn get_channels(
        &mut self,
        _num_input_channels: *mut i32,
        _num_output_channels: *mut i32,
    ) -> crate::asio::AsioResult {
        todo!()
    }

    fn get_latencies(&mut self, _input_latency: *mut i32, _output_latency: *mut i32) -> crate::asio::AsioResult {
        todo!()
    }

    fn get_buffer_size(
        &mut self,
        _min_size: *mut i32,
        _max_size: *mut i32,
        _preferred_size: *mut i32,
        _granularity: *mut i32,
    ) -> crate::asio::AsioResult {
        todo!()
    }

    fn can_sample_rate(&mut self, _sample_rate: crate::asio::SampleRate) -> crate::asio::AsioResult {
        todo!()
    }

    fn get_sample_rate(&mut self, _sample_rate: *mut crate::asio::SampleRate) -> crate::asio::AsioResult {
        todo!()
    }

    fn set_sample_rate(&mut self, _sample_rate: crate::asio::SampleRate) -> crate::asio::AsioResult {
        todo!()
    }

    fn get_clock_sources(&mut self, _clocks: *mut crate::asio::ClockSource, _num_sources: *mut i32) -> crate::asio::AsioResult {
        todo!()
    }

    fn set_clock_source(&mut self, _reference: i32) -> crate::asio::AsioResult {
        todo!()
    }

    fn get_sample_position(&mut self, _s_pos: *mut crate::asio::Samples, _t_stamp: *mut crate::asio::TimeStamp) -> crate::asio::AsioResult {
        todo!()
    }

    fn get_channel_info(&mut self, _info: *mut crate::asio::ChannelInfo) -> crate::asio::AsioResult {
        todo!()
    }

    fn create_buffers(
        &mut self,
        _buffer_infos: *mut crate::asio::BufferInfo,
        _num_channels: i32,
        _buffer_size: i32,
        _callbacks: *mut crate::asio::Callbacks,
    ) -> crate::asio::AsioResult {
        todo!()
    }

    fn dispose_buffers(&mut self) -> crate::asio::AsioResult {
        todo!()
    }

    fn control_panel(&mut self) -> crate::asio::AsioResult {
        todo!()
    }

    fn future(&mut self, _selector: i32, _opt: *mut std::ffi::c_void) -> crate::asio::AsioResult {
        todo!()
    }

    fn output_ready(&mut self) -> crate::asio::AsioResult {
        todo!()
    }
}

crate::export_asio_driver!(RWAsioDriver);