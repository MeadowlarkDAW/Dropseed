use basedrop::Shared;
use std::{collections::HashMap, error::Error};

use crate::host::HostInfo;
use crate::plugin::{PluginDescriptor, PluginFactory, PluginMainThread};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluginType {
    Internal,
    Clap,
}

pub struct ScannedPlugin {
    plugin_factory: Box<dyn PluginFactory>,
    rdn: Shared<String>,
    plugin_type: PluginType,
}

impl ScannedPlugin {
    pub fn description<'a>(&self) -> PluginDescriptor<'a> {
        self.plugin_factory.description()
    }

    pub fn plugin_type(&self) -> PluginType {
        self.plugin_type
    }

    pub fn rdn(&self) -> String {
        self.plugin_factory.description().id.to_string()
    }
}

pub struct PluginScanner {
    pub scanned_plugins: HashMap<String, ScannedPlugin>,

    coll_handle: basedrop::Handle,
}

impl PluginScanner {
    pub fn new(coll_handle: basedrop::Handle) -> Self {
        Self { scanned_plugins: HashMap::default(), coll_handle }
    }

    pub fn scan_internal_plugin(&mut self, plugin_factory: Box<dyn PluginFactory>) -> String {
        let rdn = plugin_factory.description().id.to_string();

        if self.scanned_plugins.contains_key(&rdn) {
            log::warn!("Already scanned plugin with id: {}", &rdn);
        }

        let instance = ScannedPlugin {
            plugin_factory,
            rdn: Shared::new(&self.coll_handle, rdn.clone()),
            plugin_type: PluginType::Internal,
        };

        let _ = self.scanned_plugins.insert(rdn.clone(), instance);

        rdn
    }

    pub(crate) fn new_instance(
        &mut self,
        rdn: &str,
        host_info: Shared<HostInfo>,
    ) -> Result<(Box<dyn PluginMainThread>, Shared<String>, PluginType), NewPluginInstanceError>
    {
        if let Some(plugin_factory) = self.scanned_plugins.get_mut(rdn) {
            match plugin_factory.plugin_factory.new(host_info, &self.coll_handle) {
                Ok(p) => Ok((p, Shared::clone(&plugin_factory.rdn), plugin_factory.plugin_type)),
                Err(e) => Err(NewPluginInstanceError::InstantiationError(rdn.to_string(), e)),
            }
        } else {
            Err(NewPluginInstanceError::NotFound(rdn.to_string()))
        }
    }
}

#[derive(Debug)]
pub enum NewPluginInstanceError {
    InstantiationError(String, Box<dyn Error>),
    NotFound(String),
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
        }
    }
}
