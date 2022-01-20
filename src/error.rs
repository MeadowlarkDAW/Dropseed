use std::error::Error;

use crate::clap_plugin_host::{PluginState, ThreadState};

#[derive(Debug, Clone, Copy)]
pub struct ClapPluginThreadError {
    pub requested_state: ThreadState,
    pub actual_state: ThreadState,
}

impl Error for ClapPluginThreadError {}

impl std::fmt::Display for ClapPluginThreadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Attempted to borrow CLAP plugin in an invalid thread state. Reqeusted {:?}, actual {:?}.", self.requested_state, self.actual_state)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ClapPluginActivationError {
    PluginAlreadyActivated,
    PluginNotLoaded,
    PluginFailure,
}

impl Error for ClapPluginActivationError {}

impl std::fmt::Display for ClapPluginActivationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            ClapPluginActivationError::PluginAlreadyActivated => {
                write!(
                    f,
                    "Could not activate CLAP plugin. Plugin is already activated."
                )
            }
            ClapPluginActivationError::PluginNotLoaded => {
                write!(
                    f,
                    "Could not activate CLAP plugin. Plugin has not been loaded yet."
                )
            }
            ClapPluginActivationError::PluginFailure => {
                write!(f, "CLAP plugin failed to activate.")
            }
        }
    }
}
