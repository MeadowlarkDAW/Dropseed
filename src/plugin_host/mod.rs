pub mod error;

mod channel;
mod main_thread;
mod process_thread;

pub(crate) mod event_io_buffers;
pub(crate) mod external;

pub use main_thread::{ParamModifiedInfo, PluginHostMainThread};

pub(crate) use channel::{PluginHostProcessorWrapper, SharedPluginHostProcessor};
pub(crate) use main_thread::OnIdleResult;
