use basedrop::Shared;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use walkdir::WalkDir;

use dropseed_plugin_api::plugin_scanner::{PluginFormat, ScannedPluginKey};
use dropseed_plugin_api::{
    DSPluginSaveState, HostInfo, HostRequestChannelReceiver, PluginDescriptor, PluginFactory,
    PluginInstanceID, PluginInstanceType,
};

use crate::engine::error::NewPluginInstanceError;
use crate::plugin_host::PluginHostMainThread;
use crate::utils::thread_id::SharedThreadIDs;

mod missing_plugin;
use missing_plugin::MissingPluginMainThread;

#[cfg(all(
    feature = "clap-host",
    any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd")
))]
const DEFAULT_CLAP_SCAN_DIRECTORIES: [&'static str; 2] = ["/usr/lib/clap", "/usr/local/lib/clap"];

#[cfg(all(feature = "clap-host", target_os = "macos"))]
const DEFAULT_CLAP_SCAN_DIRECTORIES: [&'static str; 1] = ["/Library/Audio/Plug-Ins/CLAP"];

#[cfg(all(feature = "clap-host", target_os = "windows"))]
// TODO: Find the proper "Common Files" folder at runtime.
const DEFAULT_CLAP_SCAN_DIRECTORIES: [&'static str; 1] = ["C:/Program Files/Common Files/CLAP"];

const MAX_SCAN_DEPTH: usize = 10;

#[derive(Debug, Clone)]
pub struct ScannedPlugin {
    pub description: PluginDescriptor,
    pub format: PluginFormat,
    pub format_version: String,
    pub key: ScannedPluginKey,
}

impl ScannedPlugin {
    pub fn rdn(&self) -> &str {
        &*self.key.rdn.as_str()
    }
}

struct ScannedPluginFactory {
    pub rdn: Shared<String>,
    pub format: PluginFormat,

    factory: Box<dyn PluginFactory>,
}

pub(crate) struct PluginScanner {
    // TODO: Use a hashmap that performs better with strings that are around 10-30
    // characters long?
    scanned_internal_plugins: HashMap<ScannedPluginKey, ScannedPluginFactory>,
    scanned_external_plugins: HashMap<ScannedPluginKey, ScannedPluginFactory>,

    #[cfg(feature = "clap-host")]
    clap_scan_directories: Vec<PathBuf>,

    host_info: Shared<HostInfo>,

    thread_ids: SharedThreadIDs,

    next_plug_unique_id: u64,

    coll_handle: basedrop::Handle,
}

impl PluginScanner {
    pub fn new(
        coll_handle: basedrop::Handle,
        host_info: Shared<HostInfo>,
        thread_ids: SharedThreadIDs,
    ) -> Self {
        Self {
            scanned_internal_plugins: HashMap::default(),
            scanned_external_plugins: HashMap::default(),

            #[cfg(any(feature = "clap-host"))]
            clap_scan_directories: Vec::new(),

            host_info,

            thread_ids,

            // IDs 0 and 1 are used exclusively by the graph_in_node and graph_out_node
            // respectively.
            next_plug_unique_id: 2,

            coll_handle,
        }
    }

    #[cfg(feature = "clap-host")]
    pub fn add_clap_scan_directory(&mut self, path: PathBuf) -> bool {
        // Check if the path is already a default path.
        for p in DEFAULT_CLAP_SCAN_DIRECTORIES.iter() {
            if path == PathBuf::from_str(p).unwrap() {
                log::warn!("Path is already a default scan directory {:?}", &path);
                return false;
            }
        }

        if !self.clap_scan_directories.contains(&path) {
            // Make sure the directory exists.
            match std::fs::read_dir(&path) {
                Ok(_) => {
                    log::info!("Added plugin scan directory {:?}", &path);
                    self.clap_scan_directories.push(path);
                    true
                }
                Err(e) => {
                    log::error!("Failed to add plugin scan directory {:?}: {}", &path, e);
                    false
                }
            }
        } else {
            log::warn!("Already added plugin scan directory {:?}", &path);
            false
        }
    }

    #[cfg(feature = "clap-host")]
    pub fn remove_clap_scan_directory(&mut self, path: PathBuf) -> bool {
        let mut remove_i = None;
        for (i, p) in self.clap_scan_directories.iter().enumerate() {
            if &path == p {
                remove_i = Some(i);
                break;
            }
        }

        if let Some(i) = remove_i {
            self.clap_scan_directories.remove(i);

            log::info!("Removed plugin scan directory {:?}", &path);

            true
        } else {
            log::warn!("Already removed plugin scan directory {:?}", &path);
            false
        }
    }

    pub fn rescan_plugin_directories(&mut self) -> RescanPluginDirectoriesRes {
        log::info!("(Re)scanning plugin directories...");

        // TODO: Detect duplicate plugins (both duplicates with different versions and with different formats)

        // TODO: Scan plugins in a separate thread?

        self.scanned_external_plugins.clear();
        let mut scanned_plugins: Vec<ScannedPlugin> = Vec::new();
        let mut failed_plugins: Vec<(PathBuf, String)> = Vec::new();

        for (key, f) in self.scanned_internal_plugins.iter() {
            scanned_plugins.push(ScannedPlugin {
                description: f.factory.description(),
                format: PluginFormat::Internal,
                format_version: env!("CARGO_PKG_VERSION").into(),
                key: key.clone(),
            })
        }

        #[cfg(feature = "clap-host")]
        {
            let mut found_binaries: Vec<PathBuf> = Vec::new();

            let mut scan_directories: Vec<PathBuf> = DEFAULT_CLAP_SCAN_DIRECTORIES
                .iter()
                .map(|s| PathBuf::from_str(s).unwrap())
                .collect();

            if let Some(mut dir) = dirs::home_dir() {
                dir.push(".clap");
                scan_directories.push(dir);
            } else {
                log::warn!("Could not search local clap plugin directory: Could not get user's home directory");
            }

            for dir in scan_directories.iter().chain(self.clap_scan_directories.iter()) {
                let walker = WalkDir::new(dir).max_depth(MAX_SCAN_DEPTH).follow_links(true);

                for item in walker {
                    match item {
                        Ok(binary) => {
                            if !binary.file_type().is_file() {
                                continue;
                            }

                            match binary.path().extension().and_then(|e| e.to_str()) {
                                Some(ext) if ext == "clap" => {}
                                _ => continue,
                            };

                            let binary_path = binary.into_path();
                            log::trace!("Found CLAP binary: {:?}", &binary_path);
                            found_binaries.push(binary_path);
                        }
                        Err(e) => {
                            log::warn!("Failed to scan binary for potential CLAP plugin: {}", e);
                        }
                    }
                }
            }

            for binary_path in found_binaries.iter() {
                match crate::plugin_host::external::clap::factory::entry_init(
                    binary_path,
                    self.thread_ids.clone(),
                    &self.coll_handle,
                ) {
                    Ok(mut factories) => {
                        for f in factories.drain(..) {
                            let id: String = f.description().id.clone();
                            let v = f.clap_version;
                            let format_version = format!("{}.{}.{}", v.major, v.minor, v.revision);

                            log::debug!(
                                "Successfully scanned CLAP plugin with ID: {}, version {}, and CLAP version {}",
                                &id,
                                &f.description().version,
                                &format_version,
                            );
                            log::trace!("Full plugin descriptor: {:?}", f.description());

                            let key =
                                ScannedPluginKey { rdn: id.clone(), format: PluginFormat::Clap };

                            let description = f.description();

                            scanned_plugins.push(ScannedPlugin {
                                description,
                                format: PluginFormat::Clap,
                                format_version,
                                key: key.clone(),
                            });

                            if self
                                .scanned_external_plugins
                                .insert(
                                    key,
                                    ScannedPluginFactory {
                                        rdn: Shared::new(&self.coll_handle, id.clone()),
                                        format: PluginFormat::Clap,
                                        factory: Box::new(f),
                                    },
                                )
                                .is_some()
                            {
                                // TODO: Handle this better
                                log::warn!("Found duplicate CLAP plugins with ID: {}", &id);
                                let _ = scanned_plugins.pop();
                            }
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to scan CLAP plugin binary at {:?}: {}",
                            binary_path,
                            e
                        );
                        failed_plugins.push((binary_path.clone(), e));
                    }
                }
            }
        }

        RescanPluginDirectoriesRes { scanned_plugins, failed_plugins }
    }

    pub fn scan_internal_plugin(
        &mut self,
        factory: Box<dyn PluginFactory>,
    ) -> Result<ScannedPluginKey, String> {
        let description = factory.description();

        let key =
            ScannedPluginKey { rdn: description.id.to_string(), format: PluginFormat::Internal };

        if self.scanned_internal_plugins.contains_key(&key) {
            log::warn!("Already scanned internal plugin: {:?}", &key);
        }

        let scanned_plugin = ScannedPluginFactory {
            factory,
            rdn: Shared::new(&self.coll_handle, key.rdn.clone()),
            format: PluginFormat::Internal,
        };

        let _ = self.scanned_internal_plugins.insert(key.clone(), scanned_plugin);

        Ok(key)
    }

    pub(crate) fn create_plugin(
        &mut self,
        mut save_state: DSPluginSaveState,
        node_ref: audio_graph::NodeRef,
        fallback_to_other_formats: bool,
    ) -> CreatePluginResult {
        // TODO: return an actual result
        let mut factory = None;
        let mut status = Ok(());

        // Always try to use internal plugins when available.
        if save_state.key.format == PluginFormat::Internal || fallback_to_other_formats {
            let res = if save_state.key.format == PluginFormat::Internal {
                self.scanned_internal_plugins.get_mut(&save_state.key)
            } else {
                let new_key = ScannedPluginKey {
                    rdn: save_state.key.rdn.clone(),
                    format: PluginFormat::Internal,
                };
                self.scanned_internal_plugins.get_mut(&new_key)
            };

            if let Some(f) = res {
                factory = Some(f);
            } else {
                status = Err(NewPluginInstanceError::FormatNotFound(
                    save_state.key.rdn.clone(),
                    PluginFormat::Internal,
                ));
            }
        }

        #[cfg(feature = "clap-host")]
        // Next try to use the clap version of the plugin.
        if factory.is_none()
            && (save_state.key.format == PluginFormat::Clap || fallback_to_other_formats)
        {
            let res = if save_state.key.format == PluginFormat::Clap {
                self.scanned_external_plugins.get_mut(&save_state.key)
            } else {
                let new_key = ScannedPluginKey {
                    rdn: save_state.key.rdn.clone(),
                    format: PluginFormat::Clap,
                };
                self.scanned_external_plugins.get_mut(&new_key)
            };

            if let Some(f) = res {
                factory = Some(f);
            } else {
                status = Err(NewPluginInstanceError::FormatNotFound(
                    save_state.key.rdn.clone(),
                    PluginFormat::Clap,
                ));
            }
        }

        let mut format = PluginInstanceType::Unloaded;

        let (host_request_rx, channel_send) = HostRequestChannelReceiver::new_channel();

        let plugin_host = if let Some(factory) = factory {
            format = factory.format.into();
            let rdn = factory.rdn.clone();

            if save_state.key.format != factory.format {
                save_state.key =
                    ScannedPluginKey { rdn: save_state.key.rdn.clone(), format: factory.format };
            }

            let id =
                PluginInstanceID::_new(node_ref.as_usize(), self.next_plug_unique_id, format, rdn);
            self.next_plug_unique_id += 1;

            let plug_main_thread = match factory.factory.instantiate(
                channel_send,
                self.host_info.clone(),
                id.clone(),
                &self.coll_handle,
            ) {
                Ok(plug_main_thread) => {
                    status = Ok(());

                    plug_main_thread
                }
                Err(e) => {
                    status = Err(NewPluginInstanceError::FactoryFailedToCreateNewInstance(
                        (*factory.rdn).clone(),
                        e,
                    ));

                    Box::new(MissingPluginMainThread::new(
                        save_state.key.clone(),
                        save_state.backup_audio_ports.clone(),
                        save_state.backup_note_ports.clone(),
                    ))
                }
            };

            PluginHostMainThread::new(id, save_state, plug_main_thread, host_request_rx)
        } else {
            let rdn = Shared::new(&self.coll_handle, save_state.key.rdn.clone());

            let id =
                PluginInstanceID::_new(node_ref.as_usize(), self.next_plug_unique_id, format, rdn);
            self.next_plug_unique_id += 1;

            if status.is_ok() {
                status = Err(NewPluginInstanceError::NotFound(save_state.key.rdn.clone()));
            }

            let plug_main_thread = Box::new(MissingPluginMainThread::new(
                save_state.key.clone(),
                save_state.backup_audio_ports.clone(),
                save_state.backup_note_ports.clone(),
            ));

            PluginHostMainThread::new(id, save_state, plug_main_thread, host_request_rx)
        };

        CreatePluginResult { plugin_host, status }
    }
}

pub(crate) struct CreatePluginResult {
    pub plugin_host: PluginHostMainThread,
    pub status: Result<(), NewPluginInstanceError>,
}

#[derive(Debug)]
pub struct RescanPluginDirectoriesRes {
    pub scanned_plugins: Vec<ScannedPlugin>,
    pub failed_plugins: Vec<(PathBuf, String)>,
}
