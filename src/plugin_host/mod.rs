pub mod events;

mod channel;
mod error;
mod main_thread;
mod process_thread;

pub use error::ActivatePluginError;
pub use main_thread::ParamModifiedInfo;

pub(crate) use channel::SharedPluginHostProcThread;
pub(crate) use main_thread::{OnIdleResult, PluginHostMainThread};
