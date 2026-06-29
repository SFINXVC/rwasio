use crate::ApplicationResult;

pub mod pipewire;

#[derive(Debug, Clone)]
pub struct AudioDevice {
    pub name: String,
    pub id: String,
}

pub struct DuplexConfig {
    pub sample_rate: f64,
    pub buffer_size: u32,
    pub input_channels: u32,
    pub output_channels: u32,
    pub output_target: Option<u32>,
    pub input_target: Option<u32>,
}

pub type ProcessCallback = Box<dyn FnMut(&[&[f32]], &mut [&mut [f32]]) + Send + 'static>;

pub trait AudioBackend: Send {
    fn name(&self) -> &'static str;

    fn sinks(&self) -> Vec<AudioDevice>;
    fn sources(&self) -> Vec<AudioDevice>;

    fn start(&mut self, config: DuplexConfig, process: ProcessCallback) -> ApplicationResult<()>;
    fn stop(&mut self) -> ApplicationResult<()>;

    fn set_output_target(&mut self, id: Option<u32>) -> ApplicationResult<()>;
    fn set_input_target(&mut self, id: Option<u32>) -> ApplicationResult<()>;
}
