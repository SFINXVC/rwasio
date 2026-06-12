use crate::ApplicationResult;

pub mod pipewire;

#[derive(Debug, Clone)]
pub struct AudioDevice {
    pub name: String,
    pub id: String,
}

pub struct StreamConfig {
    pub sample_rate: f64,
    pub buffer_size: u32,
    pub channels: u32,
}

pub trait AudioBackend: Send {
    fn name(&self) -> &'static str;

    fn sinks(&self) -> Vec<AudioDevice>;
    fn sources(&self) -> Vec<AudioDevice>;

    fn start_output(
        &mut self,
        device_id: &str,
        config: StreamConfig,
        process: Box<dyn Fn(&mut [f32]) + Send + 'static>,
    ) -> ApplicationResult<()>;

    fn stop_output(&mut self) -> ApplicationResult<()>;

    fn start_input(
        &mut self,
        device_id: &str,
        config: StreamConfig,
        process: Box<dyn Fn(&[f32]) + Send + 'static>,
    ) -> ApplicationResult<()>;

    fn stop_input(&mut self) -> ApplicationResult<()>;
}
