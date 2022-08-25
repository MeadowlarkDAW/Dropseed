use std::error::Error;

use dropseed_plugin_api::ParamID;

pub use clack_extensions::gui::GuiError;

#[derive(Debug)]
pub enum ActivatePluginError {
    NotLoaded,
    AlreadyActive,
    RestartScheduled,
    PluginFailedToGetAudioPortsExt(String),
    PluginFailedToGetNotePortsExt(String),
    PluginFailedToGetParamInfo(usize),
    PluginFailedToGetParamValue(ParamID),
    PluginSpecific(String),
}

impl Error for ActivatePluginError {}

impl std::fmt::Display for ActivatePluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActivatePluginError::NotLoaded => write!(f, "plugin failed to load from disk"),
            ActivatePluginError::AlreadyActive => write!(f, "plugin is already active"),
            ActivatePluginError::RestartScheduled => {
                write!(f, "a restart is scheduled for this plugin")
            }
            ActivatePluginError::PluginFailedToGetAudioPortsExt(e) => {
                write!(f, "plugin returned error while getting audio ports extension: {:?}", e)
            }
            ActivatePluginError::PluginFailedToGetNotePortsExt(e) => {
                write!(f, "plugin returned error while getting note ports extension: {:?}", e)
            }
            ActivatePluginError::PluginFailedToGetParamInfo(index) => {
                write!(f, "plugin returned error while getting parameter info at index: {}", index)
            }
            ActivatePluginError::PluginFailedToGetParamValue(param_id) => {
                write!(
                    f,
                    "plugin returned error while getting parameter value with ID: {:?}",
                    param_id
                )
            }
            ActivatePluginError::PluginSpecific(e) => {
                write!(f, "plugin returned error while activating: {:?}", e)
            }
        }
    }
}

impl From<String> for ActivatePluginError {
    fn from(e: String) -> Self {
        ActivatePluginError::PluginSpecific(e)
    }
}

#[derive(Debug, Clone)]
pub enum SetParamValueError {
    ParamDoesNotExist(ParamID),
    PluginNotActive,
    ParamIsReadOnly(ParamID),
    ParamIsNotModulatable(ParamID),
}

impl Error for SetParamValueError {}

impl std::fmt::Display for SetParamValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SetParamValueError::ParamDoesNotExist(id) => {
                write!(f, "failed to set value of plugin parameter: parameter with id {:?} does not exist", id)
            }
            SetParamValueError::PluginNotActive => {
                write!(f, "failed to set value of plugin parameter: plugin is not activated")
            }
            SetParamValueError::ParamIsReadOnly(id) => {
                write!(
                    f,
                    "failed to set value of plugin parameter: parameter with id {:?} is read-only",
                    id
                )
            }
            SetParamValueError::ParamIsNotModulatable(id) => {
                write!(
                    f,
                    "failed to set modulation amount on plugin parameter: parameter with id {:?} is not marked as modulatable",
                    id
                )
            }
        }
    }
}

#[derive(Debug)]
pub enum ShowGuiError {
    HostError(GuiError),
    AlreadyOpen,
}

impl Error for ShowGuiError {}

impl std::fmt::Display for ShowGuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShowGuiError::HostError(e) => {
                write!(f, "Failed to open plugin GUI: {}", e)
            }
            ShowGuiError::AlreadyOpen => {
                write!(f, "Failed to open plugin GUI: plugin GUI is already open")
            }
        }
    }
}