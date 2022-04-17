use basedrop::Shared;
use std::collections::hash_map;
use std::{collections::HashMap, error::Error};

use crate::host::HostInfo;
use crate::plugin::{PluginDescriptor, PluginFactory, PluginMainThread};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginFormat {
    Internal,
    Clap,
}

pub struct ScannedPlugin {
    pub description: PluginDescriptor,
    pub rdn: Shared<String>,
    pub format: PluginFormat,

    factory: Box<dyn PluginFactory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScannedPluginKey {
    pub rdn: String,
    pub format: PluginFormat,
}

pub struct PluginScanner {
    scanned_plugins: HashMap<ScannedPluginKey, ScannedPlugin>,

    coll_handle: basedrop::Handle,
}

impl PluginScanner {
    pub fn new(coll_handle: basedrop::Handle) -> Self {
        Self { scanned_plugins: HashMap::default(), coll_handle }
    }

    pub fn scanned_plugins(&self) -> hash_map::Iter<ScannedPluginKey, ScannedPlugin> {
        self.scanned_plugins.iter()
    }

    pub fn scan_internal_plugin(
        &mut self,
        mut factory: Box<dyn PluginFactory>,
    ) -> Result<ScannedPluginKey, Box<dyn Error>> {
        let description = match factory.entry_init(None) {
            Ok(d) => d,
            Err(e) => {
                log::error!("Error while scanning internal plugin: {}", &e);
                return Err(e);
            }
        };

        let key =
            ScannedPluginKey { rdn: description.id.to_string(), format: PluginFormat::Internal };

        if self.scanned_plugins.contains_key(&key) {
            log::warn!("Already scanned internal plugin: {:?}", &key);
        }

        let scanned_plugin = ScannedPlugin {
            factory,
            description,
            rdn: Shared::new(&self.coll_handle, key.rdn.clone()),
            format: PluginFormat::Internal,
        };

        let _ = self.scanned_plugins.insert(key.clone(), scanned_plugin);

        Ok(key)
    }

    pub(crate) fn create_plugin(
        &mut self,
        key: &ScannedPluginKey,
        host_info: Shared<HostInfo>,
        fallback_to_other_formats: bool,
    ) -> Result<(Box<dyn PluginMainThread>, Shared<String>, PluginFormat), NewPluginInstanceError>
    {
        if let Some(factory) = self.scanned_plugins.get_mut(key) {
            match factory.factory.new(host_info, &self.coll_handle) {
                Ok(p) => Ok((p, Shared::clone(&factory.rdn), factory.format)),
                Err(e) => Err(NewPluginInstanceError::InstantiationError(key.rdn.clone(), e)),
            }
        } else {
            // First check if the plugin has an internal format.
            if key.format != PluginFormat::Internal {
                let internal_key =
                    ScannedPluginKey { rdn: key.rdn.clone(), format: PluginFormat::Internal };

                if let Some(factory) = self.scanned_plugins.get_mut(&internal_key) {
                    if fallback_to_other_formats {
                        match factory.factory.new(host_info, &self.coll_handle) {
                            Ok(p) => {
                                return Ok((p, Shared::clone(&factory.rdn), PluginFormat::Internal))
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

                if let Some(factory) = self.scanned_plugins.get_mut(&clap_key) {
                    if fallback_to_other_formats {
                        match factory.factory.new(host_info, &self.coll_handle) {
                            Ok(p) => {
                                return Ok((p, Shared::clone(&factory.rdn), PluginFormat::Clap))
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
