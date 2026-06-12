use std::sync::OnceLock;
use std::sync::atomic::AtomicU32;

use thiserror::Error;

use crate::ApplicationError::BackendInitializationFailed;

pub mod asio;
pub mod backends;
pub mod com;
pub mod driver;
pub mod gui;

pub type ApplicationResult<T> = Result<T, ApplicationError>;

pub type DeviceList = (Vec<(String, String)>, Vec<(String, String)>);
pub static DEVICE_LIST: OnceLock<DeviceList> = OnceLock::new();
pub static SELECTED_SINK: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());
pub static SELECTED_SOURCE: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());
pub static ACTIVE_BACKEND: OnceLock<
    std::sync::Mutex<Box<dyn crate::backends::AudioBackend + Send>>,
> = OnceLock::new();

pub enum PwStreamCmd {
    Stop,
    SetTarget(Option<u32>),
}

pub static PW_STREAM_SENDER: std::sync::RwLock<Option<pipewire::channel::Sender<PwStreamCmd>>> =
    std::sync::RwLock::new(None);

pub static DBG_ASIO_BUFFER_SIZE: AtomicU32 = AtomicU32::new(0);
pub static DBG_SAMPLE_RATE: AtomicU32 = AtomicU32::new(44100);
pub static DBG_NUM_INPUTS: AtomicU32 = AtomicU32::new(0);
pub static DBG_NUM_OUTPUTS: AtomicU32 = AtomicU32::new(0);
pub static DBG_CURRENT_BUFFER_IDX: AtomicU32 = AtomicU32::new(0);
pub static DBG_STAGING_SAMPLES: AtomicU32 = AtomicU32::new(0);

pub static PW_INPUT_STREAM_SENDER: std::sync::RwLock<
    Option<pipewire::channel::Sender<PwStreamCmd>>,
> = std::sync::RwLock::new(None);

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("An error occurred while trying to initialize the {0} backend: {1}")]
    BackendInitializationFailed(String, String),

    #[error("An unknown error occurred: {0}")]
    Unknown(String),
}

impl From<pipewire::Error> for ApplicationError {
    fn from(value: pipewire::Error) -> Self {
        let reason = value.to_string();
        BackendInitializationFailed("pipewire".to_string(), reason)
    }
}

#[macro_export]
macro_rules! rlog {
    ($($arg:tt)*) => {{
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/home/sfinxv/.var/app/com.usebottles.bottles/data/bottles/bottles/FL-Studio__110/drive_c/rwasio.log")
        {
            let _ = writeln!(f, $($arg)*);
        }
    }};
}
