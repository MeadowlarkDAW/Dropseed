use basedrop::Shared;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::{collections::HashMap, error::Error};

use crate::host_request::HostRequest;
use crate::plugin::{PluginDescriptor, PluginFactory, PluginMainThread, PluginSaveState};

#[cfg(feature = "clap-host")]
use crate::clap::plugin::ClapPluginFactory;

#[cfg(feature = "clap-host")]
const DEFAULT_CLAP_SCAN_DIRECTORIES: [&'static str; 3] =
    ["~/.clap", "/usr/lib/clap", "/usr/local/lib/clap"];

const MAX_SCAN_DEPTH: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginFormat {
    Internal,
    Clap,
}

impl std::fmt::Display for PluginFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginFormat::Internal => {
                write!(f, "internal")
            }
            PluginFormat::Clap => {
                write!(f, "clap")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScannedPlugin {
    pub description: PluginDescriptor,
    pub format: PluginFormat,
    pub format_version: Option<String>,
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
    scanned_internal_plugins: HashMap<ScannedPluginKey, ScannedPluginFactory>,
    scanned_external_plugins: HashMap<ScannedPluginKey, ScannedPluginFactory>,

    #[cfg(feature = "clap-host")]
    clap_scan_directories: Vec<PathBuf>,

    coll_handle: basedrop::Handle,
}

impl PluginScanner {
    pub fn new(coll_handle: basedrop::Handle) -> Self {
        Self {
            scanned_internal_plugins: HashMap::default(),
            scanned_external_plugins: HashMap::default(),

            #[cfg(any(feature = "clap-host"))]
            clap_scan_directories: Vec::new(),

            coll_handle,
        }
    }

    #[cfg(feature = "clap-host")]
    pub fn add_clap_scan_directory(&mut self, path: PathBuf) -> bool {
        use std::str::FromStr;

        // Check if the path is already a default path.
        for p in DEFAULT_CLAP_SCAN_DIRECTORIES.iter() {
            if &path == &PathBuf::from_str(p).unwrap() {
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
        let mut failed_plugins: Vec<(PathBuf, Box<dyn Error>)> = Vec::new();

        #[cfg(feature = "clap-host")]
        {
            let mut found_binaries: Vec<PathBuf> = Vec::new();

            #[cfg(any(
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd",
                target_os = "netbsd"
            ))]
            const CLAP_SEARCH_EXT: [&'static str; 1] = ["*.{clap}"];

            for dir in DEFAULT_CLAP_SCAN_DIRECTORIES
                .map(|s| PathBuf::from_str(s).unwrap())
                .iter()
                .chain(self.clap_scan_directories.iter())
            {
                match globwalk::GlobWalkerBuilder::from_patterns(dir, &CLAP_SEARCH_EXT)
                    .max_depth(MAX_SCAN_DEPTH)
                    .follow_links(true)
                    .build()
                {
                    Ok(walker) => {
                        for item in walker.into_iter() {
                            match item {
                                Ok(binary) => {
                                    let binary_path = binary.into_path();
                                    log::trace!("Found CLAP binary: {:?}", &binary_path);
                                    found_binaries.push(binary_path);
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to scan binary for potential CLAP plugin: {}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to walk directory {:?} for CLAP plugins: {}", dir, e);
                    }
                }
            }

            for binary_path in found_binaries.iter() {
                match ClapPluginFactory::entry_init(binary_path, &self.coll_handle) {
                    Ok(mut factories) => {
                        for f in factories.drain(..) {
                            let id: String = f.descriptor().id.clone();
                            let v = f.clap_version();
                            let format_version = format!("{}.{}.{}", v.major, v.minor, v.revision);

                            log::info!(
                                "Successfully scanned CLAP plugin with ID: {}, version {}, and CLAP version {}",
                                &id,
                                &f.descriptor().version,
                                &format_version,
                            );
                            log::trace!("Full plugin descriptor: {:?}", f.descriptor());

                            let key =
                                ScannedPluginKey { rdn: id.clone(), format: PluginFormat::Clap };

                            scanned_plugins.push(ScannedPlugin {
                                description: f.description(),
                                format: PluginFormat::Clap,
                                format_version: Some(format_version),
                                key: key.clone(),
                            });

                            if let Some(_) = self.scanned_external_plugins.insert(
                                key,
                                ScannedPluginFactory {
                                    description: f.description(),
                                    rdn: Shared::new(&self.coll_handle, id.clone()),
                                    format: PluginFormat::Clap,
                                    factory: Box::new(f),
                                },
                            ) {
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
    ) -> Result<ScannedPluginKey, Box<dyn Error>> {
        let description = factory.description();

        let key =
            ScannedPluginKey { rdn: description.id.to_string(), format: PluginFormat::Internal };

        if self.scanned_internal_plugins.contains_key(&key) {
            log::warn!("Already scanned internal plugin: {:?}", &key);
        }

        let scanned_plugin = ScannedPluginFactory {
            factory,
            description,
            rdn: Shared::new(&self.coll_handle, key.rdn.clone()),
            format: PluginFormat::Internal,
        };

        let _ = self.scanned_internal_plugins.insert(key.clone(), scanned_plugin);

        Ok(key)
    }

    pub(crate) fn create_plugin(
        &mut self,
        key: &ScannedPluginKey,
        host_request: &HostRequest,
        activation_requested: bool,
        fallback_to_other_formats: bool,
    ) -> Result<
        (Box<dyn PluginMainThread>, Shared<String>, PluginFormat, PluginSaveState),
        NewPluginInstanceError,
    > {
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

        let mut try_other_formats = false;

        #[cfg(not(feature = "clap-host"))]
        if key.format == PluginFormat::Clap && fallback_to_other_formats {
            try_other_formats = true;
        }

        if key.format == PluginFormat::Internal || try_other_formats {
            let res = if key.format == PluginFormat::Internal {
                self.scanned_internal_plugins.get_mut(key)
            } else {
                let new_key =
                    ScannedPluginKey { rdn: key.rdn.clone(), format: PluginFormat::Internal };
                self.scanned_internal_plugins.get_mut(&new_key)
            };

            if let Some(factory) = res {
                let res = match factory.factory.new(host_request, &self.coll_handle) {
                    Ok(p) => {
                        let save_state = PluginSaveState {
                            key: key.clone(),
                            activation_requested,
                            audio_in_out_channels: (0, 0),
                            _preset: (),
                        };

                        Ok((p, Shared::clone(&factory.rdn), factory.format, save_state))
                    }
                    Err(e) => Err(NewPluginInstanceError::InstantiationError(key.rdn.clone(), e)),
                };
                check_for_invalid_host_callbacks(host_request, &factory.rdn);

                return res;
            } else if fallback_to_other_formats {
                try_other_formats = true;
            } else {
                return Err(NewPluginInstanceError::FormatNotFound(
                    key.rdn.clone(),
                    PluginFormat::Internal,
                ));
            }
        }

        // Next try the CLAP format
        #[cfg(feature = "clap-host")]
        if key.format == PluginFormat::Clap || try_other_formats {
            let res = if key.format == PluginFormat::Clap {
                self.scanned_external_plugins.get_mut(key)
            } else {
                let new_key = ScannedPluginKey { rdn: key.rdn.clone(), format: PluginFormat::Clap };
                self.scanned_external_plugins.get_mut(&new_key)
            };

            if let Some(factory) = res {
                let res = match factory.factory.new(host_request, &self.coll_handle) {
                    Ok(p) => {
                        let save_state = PluginSaveState {
                            key: key.clone(),
                            activation_requested,
                            audio_in_out_channels: (0, 0),
                            _preset: (),
                        };

                        Ok((p, Shared::clone(&factory.rdn), factory.format, save_state))
                    }
                    Err(e) => Err(NewPluginInstanceError::InstantiationError(key.rdn.clone(), e)),
                };
                check_for_invalid_host_callbacks(host_request, &factory.rdn);
                return res;
            } else if fallback_to_other_formats {
                //try_other_formats = true;
            } else {
                return Err(NewPluginInstanceError::FormatNotFound(
                    key.rdn.clone(),
                    PluginFormat::Internal,
                ));
            }
        }

        Err(NewPluginInstanceError::NotFound(key.rdn.clone()))
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
