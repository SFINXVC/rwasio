use crate::ApplicationResult;

pub mod pipewire;

#[derive(Debug, Clone)]
pub struct AudioDevice {
    pub name: String,
    pub id: String,
}

#[derive(Clone)]
pub struct DuplexConfig {
    pub sample_rate: f64,
    pub buffer_size: u32,
    pub input_channels: u32,
    pub output_channels: u32,
    pub output_target: Option<u32>,
    pub input_target: Option<u32>,
}

/// A view into one channel's samples for a single process cycle.
/// `ptr` may be null iff `len == 0`. `len` is in f32 frames.
#[derive(Clone, Copy)]
pub struct InPort {
    pub ptr: *const f32,
    pub len: usize,
}

#[derive(Clone, Copy)]
pub struct OutPort {
    pub ptr: *mut f32,
    pub len: usize,
}

pub type ProcessCallback = Box<dyn FnMut(&[InPort], &mut [OutPort]) + Send + 'static>;

pub trait AudioBackend: Send {
    fn name(&self) -> &'static str;

    fn sinks(&self) -> Vec<AudioDevice>;
    fn sources(&self) -> Vec<AudioDevice>;

    fn start(&mut self, config: DuplexConfig, process: ProcessCallback) -> ApplicationResult<()>;
    fn stop(&mut self) -> ApplicationResult<()>;
}
