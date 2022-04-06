use basedrop::Shared;
use std::{collections::HashMap, error::Error};

use crate::host::HostInfo;
use crate::plugin::{PluginDescriptor, PluginFactory, PluginMainThread};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginFormat {
    Internal,
    Clap,
}

pub struct ScannedPlugin {
    plugin_factory: Box<dyn PluginFactory>,
    rdn: Shared<String>,
    format: PluginFormat,
}

impl ScannedPlugin {
    pub fn description(&self) -> &PluginDescriptor {
        self.plugin_factory.description()
    }

    pub fn format(&self) -> PluginFormat {
        self.format
    }

    pub fn rdn(&self) -> String {
        self.plugin_factory.description().id.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScannedPluginKey {
    pub rdn: String,
    pub format: PluginFormat,
}

pub struct PluginScanner {
    pub scanned_plugins: HashMap<ScannedPluginKey, ScannedPlugin>,

    coll_handle: basedrop::Handle,
}

impl PluginScanner {
    pub fn new(coll_handle: basedrop::Handle) -> Self {
        Self { scanned_plugins: HashMap::default(), coll_handle }
    }

    pub fn scan_internal_plugin(
        &mut self,
        plugin_factory: Box<dyn PluginFactory>,
    ) -> ScannedPluginKey {
        let key = ScannedPluginKey {
            rdn: plugin_factory.description().id.to_string(),
            format: PluginFormat::Internal,
        };

        if self.scanned_plugins.contains_key(&key) {
            log::warn!("Already scanned plugin: {:?}", &key);
        }

        let instance = ScannedPlugin {
            plugin_factory,
            rdn: Shared::new(&self.coll_handle, key.rdn.clone()),
            format: PluginFormat::Internal,
        };

        let _ = self.scanned_plugins.insert(key.clone(), instance);

        key
    }

    pub(crate) fn new_instance(
        &mut self,
        key: &ScannedPluginKey,
        host_info: Shared<HostInfo>,
        fallback_to_other_formats: bool,
    ) -> Result<(Box<dyn PluginMainThread>, Shared<String>, PluginFormat), NewPluginInstanceError>
    {
        if let Some(plugin_factory) = self.scanned_plugins.get_mut(key) {
            match plugin_factory.plugin_factory.new(host_info, &self.coll_handle) {
                Ok(p) => Ok((p, Shared::clone(&plugin_factory.rdn), plugin_factory.format)),
                Err(e) => Err(NewPluginInstanceError::InstantiationError(key.rdn.clone(), e)),
            }
        } else {
            // First check if the plugin has an internal format.
            if key.format != PluginFormat::Internal {
                let internal_key =
                    ScannedPluginKey { rdn: key.rdn.clone(), format: PluginFormat::Internal };

                if let Some(plugin_factory) = self.scanned_plugins.get_mut(&internal_key) {
                    if fallback_to_other_formats {
                        match plugin_factory.plugin_factory.new(host_info, &self.coll_handle) {
                            Ok(p) => {
                                return Ok((
                                    p,
                                    Shared::clone(&plugin_factory.rdn),
                                    PluginFormat::Internal,
                                ))
                            }
                            Err(e) => {
                                return Err(NewPluginInstanceError::InstantiationError(
                                    key.rdn.clone(),
                                    e,
                                ))
                            }
                        }
                    } else {
                        return Err(NewPluginInstanceError::FormatNotFound(
                            key.rdn.clone(),
                            key.format,
                        ));
                    }
                }
            }

            // Next check if the plugin has a CLAP format.
            if key.format != PluginFormat::Clap {
                let clap_key =
                    ScannedPluginKey { rdn: key.rdn.clone(), format: PluginFormat::Clap };

                if let Some(plugin_factory) = self.scanned_plugins.get_mut(&clap_key) {
                    if fallback_to_other_formats {
                        match plugin_factory.plugin_factory.new(host_info, &self.coll_handle) {
                            Ok(p) => {
                                return Ok((
                                    p,
                                    Shared::clone(&plugin_factory.rdn),
                                    PluginFormat::Clap,
                                ))
                            }
                            Err(e) => {
                                return Err(NewPluginInstanceError::InstantiationError(
                                    key.rdn.clone(),
                                    e,
                                ))
                            }
                        }
                    } else {
                        return Err(NewPluginInstanceError::FormatNotFound(
                            key.rdn.clone(),
                            key.format,
                        ));
                    }
                }
            }

            Err(NewPluginInstanceError::NotFound(key.rdn.clone()))
        }
    }
}

#[derive(Debug)]
pub enum NewPluginInstanceError {
    InstantiationError(String, Box<dyn Error>),
    NotFound(String),
    FormatNotFound(String, PluginFormat),
}

impl Error for NewPluginInstanceError {}

impl std::fmt::Display for NewPluginInstanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NewPluginInstanceError::InstantiationError(n, e) => {
                write!(f, "Failed to create instance of plugin {}: {}", n, e)
            }
            NewPluginInstanceError::NotFound(n) => {
                write!(
                    f,
                    "Failed to create instance of plugin {}: not in list of scanned plugins",
                    n
                )
            }
            NewPluginInstanceError::FormatNotFound(n, p) => {
                write!(
                    f,
                    "Failed to create instance of plugin {}: the format {:?} not found for this plugin",
                    n,
                    p
                )
            }
        }
    }
}
