use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32};

use thiserror::Error;

use crate::ApplicationError::BackendInitializationFailed;

pub mod asio;
pub mod backends;
pub mod com;
pub mod driver;
pub mod gui;

pub type ApplicationResult<T> = Result<T, ApplicationError>;

pub type DeviceList = (Vec<(String, String)>, Vec<(String, String)>);
pub static DEVICE_LIST: std::sync::RwLock<DeviceList> =
    std::sync::RwLock::new((Vec::new(), Vec::new()));
pub static SELECTED_SINK: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());
pub static SELECTED_SOURCE: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());
pub static ACTIVE_BACKEND: OnceLock<
    std::sync::Mutex<Box<dyn crate::backends::AudioBackend + Send>>,
> = OnceLock::new();

pub static STREAM_RUNNING: AtomicBool = AtomicBool::new(false);
pub static DEVICE_GENERATION: AtomicU32 = AtomicU32::new(0);
pub static MODULE_PINNED: AtomicBool = AtomicBool::new(false);

pub static DBG_ASIO_BUFFER_SIZE: AtomicU32 = AtomicU32::new(0);
pub static DBG_SAMPLE_RATE: AtomicU32 = AtomicU32::new(44100);
pub static DBG_NUM_INPUTS: AtomicU32 = AtomicU32::new(0);
pub static DBG_NUM_OUTPUTS: AtomicU32 = AtomicU32::new(0);
pub static DBG_CURRENT_BUFFER_IDX: AtomicU32 = AtomicU32::new(0);

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

fn log_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RWASIO_LOG") {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("rwasio").join("rwasio.log"))
}

pub fn init_logging() {
    let Some(path) = log_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .with_target(true)
        .with_max_level(tracing::Level::TRACE)
        .try_init();
}
