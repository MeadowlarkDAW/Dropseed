use basedrop::Shared;
use crossbeam::channel::Sender;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::{collections::HashMap, error::Error};

use crate::event::{DAWEngineEvent, PluginScannerEvent};
use crate::host::{HostInfo, HostRequest};
use crate::plugin::{PluginDescriptor, PluginFactory, PluginMainThread};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginFormat {
    Internal,
    Clap,
}

#[derive(Debug, Clone)]
pub struct ScannedPlugin {
    pub description: PluginDescriptor,
    pub key: ScannedPluginKey,
}

struct ScannedPluginFactory {
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

pub(crate) struct PluginScanner {
    // TODO: Use a hashmap that performs better with strings that are around 10-30
    // characters long?
    scanned_plugins: HashMap<ScannedPluginKey, ScannedPluginFactory>,
    plugin_scan_directories: Vec<PathBuf>,

    coll_handle: basedrop::Handle,
}

impl PluginScanner {
    pub fn new(coll_handle: basedrop::Handle) -> Self {
        Self {
            scanned_plugins: HashMap::default(),
            plugin_scan_directories: Vec::new(),
            coll_handle,
        }
    }

    pub fn add_plugin_scan_directory(&mut self, path: PathBuf) -> bool {
        if !self.plugin_scan_directories.contains(&path) {
            log::info!("Added plugin scan directory {:?}", &path);

            self.plugin_scan_directories.push(path);
            true
        } else {
            false
        }
    }

    pub fn remove_plugin_scan_directory(&mut self, path: PathBuf) -> bool {
        let mut remove_i = None;
        for (i, p) in self.plugin_scan_directories.iter().enumerate() {
            if &path == p {
                remove_i = Some(i);
                break;
            }
        }

        if let Some(i) = remove_i {
            self.plugin_scan_directories.remove(i);

            log::info!("Removed plugin scan directory {:?}", &path);

            true
        } else {
            false
        }
    }

    pub fn rescan_plugin_directories(&mut self, event_tx: &mut Sender<DAWEngineEvent>) {
        log::info!("Rescanning plugin directories...");

        // TODO
        //
        // Preferrably we should scan plugins in a separate thread.
        let res =
            RescanPluginDirectoriesRes { scanned_plugins: Vec::new(), failed_plugins: Vec::new() };
        event_tx.send(PluginScannerEvent::RescanFinished(res).into()).unwrap();
    }

    pub fn scan_internal_plugin(
        &mut self,
        mut factory: Box<dyn PluginFactory>,
    ) -> Result<ScannedPluginKey, Box<dyn Error>> {
        let description = factory.description();

        let key =
            ScannedPluginKey { rdn: description.id.to_string(), format: PluginFormat::Internal };

        if self.scanned_plugins.contains_key(&key) {
            log::warn!("Already scanned internal plugin: {:?}", &key);
        }

        let scanned_plugin = ScannedPluginFactory {
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
        host_request: &HostRequest,
        fallback_to_other_formats: bool,
    ) -> Result<(Box<dyn PluginMainThread>, Shared<String>, PluginFormat), NewPluginInstanceError>
    {
        let check_for_invalid_host_callbacks = |host_request: &HostRequest, id: &String| {
            if host_request.plugin_channel.restart_requested.load(Ordering::Relaxed) {
                host_request.plugin_channel.restart_requested.store(false, Ordering::Relaxed);
                log::warn!("Plugin with ID {} attempted to call host_request.request_restart() during PluginFactory::new(). Request was ignored.", id);
            }
            if host_request.plugin_channel.process_requested.load(Ordering::Relaxed) {
                host_request.plugin_channel.process_requested.store(false, Ordering::Relaxed);
                log::warn!("Plugin with ID {} attempted to call host_request.request_process() during PluginFactory::new(). Request was ignored.", id);
            }
            if host_request.plugin_channel.callback_requested.load(Ordering::Relaxed) {
                host_request.plugin_channel.callback_requested.store(false, Ordering::Relaxed);
                log::warn!("Plugin with ID {} attempted to call host_request.request_callback() during PluginFactory::new(). Request was ignored.", id);
            }
        };

        if let Some(factory) = self.scanned_plugins.get_mut(key) {
            let res = match factory.factory.new(host_request, &self.coll_handle) {
                Ok(p) => Ok((p, Shared::clone(&factory.rdn), factory.format)),
                Err(e) => Err(NewPluginInstanceError::InstantiationError(key.rdn.clone(), e)),
            };
            check_for_invalid_host_callbacks(host_request, &factory.rdn);
            res
        } else {
            // First check if the plugin has an internal format.
            if key.format != PluginFormat::Internal {
                let internal_key =
                    ScannedPluginKey { rdn: key.rdn.clone(), format: PluginFormat::Internal };

                if let Some(factory) = self.scanned_plugins.get_mut(&internal_key) {
                    if fallback_to_other_formats {
                        let res = match factory.factory.new(host_request, &self.coll_handle) {
                            Ok(p) => Ok((p, Shared::clone(&factory.rdn), PluginFormat::Internal)),
                            Err(e) => {
                                Err(NewPluginInstanceError::InstantiationError(key.rdn.clone(), e))
                            }
                        };
                        check_for_invalid_host_callbacks(host_request, &factory.rdn);
                        return res;
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
                        let res = match factory.factory.new(host_request, &self.coll_handle) {
                            Ok(p) => Ok((p, Shared::clone(&factory.rdn), PluginFormat::Clap)),
                            Err(e) => {
                                Err(NewPluginInstanceError::InstantiationError(key.rdn.clone(), e))
                            }
                        };
                        check_for_invalid_host_callbacks(host_request, &factory.rdn);
                        return res;
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
pub struct RescanPluginDirectoriesRes {
    pub scanned_plugins: Vec<ScannedPlugin>,
    pub failed_plugins: Vec<(PathBuf, Box<dyn Error>)>,
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
