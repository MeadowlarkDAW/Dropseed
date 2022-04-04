use std::sync::atomic::{AtomicBool, Ordering};
use std::{cell::UnsafeCell, hash::Hash};

use basedrop::Shared;
use fnv::FnvHashMap;

use crate::host::Host;
use crate::plugin::{PluginAudioThread, PluginMainThread};
use crate::ProcessStatus;

/// Used for debugging and verifying purposes.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginInstanceType {
    Internal,
    Sum,
    DelayComp,
}

impl std::fmt::Debug for PluginInstanceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                PluginInstanceType::Internal => "Int",
                PluginInstanceType::Sum => "Sum",
                PluginInstanceType::DelayComp => "Dly",
            }
        )
    }
}

/// Used to uniquely identify a plugin instance and for debugging purposes.
#[derive(Clone, Copy)]
pub struct PluginInstanceID {
    id: u64,
    plugin_type: PluginInstanceType,
    name: &'static str,
}

impl PluginInstanceID {
    pub fn new(accumulator: &mut u64, plugin_type: PluginInstanceType, name: &'static str) -> Self {
        *accumulator += 1;

        Self { id: *accumulator, plugin_type, name }
    }
}

impl std::fmt::Debug for PluginInstanceID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.plugin_type {
            PluginInstanceType::Internal => {
                write!(f, "Int({})_{}", self.name, self.id)
            }
            _ => {
                write!(f, "{:?}_{}", self.plugin_type, self.id)
            }
        }
    }
}

impl PartialEq for PluginInstanceID {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl Eq for PluginInstanceID {}

impl Hash for PluginInstanceID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

/// Used to sync plugin state between the main thread and audio thread counterparts.
pub(crate) struct PluginInstanceChannel {
    pub restart_requested: AtomicBool,
    pub process_requested: AtomicBool,
    pub callback_requested: AtomicBool,

    active: AtomicBool,
    // TODO: parameter stuff
}

pub(crate) struct PluginMainThreadInstance {
    pub plugin: Box<dyn PluginMainThread>,
    pub channel: Shared<PluginInstanceChannel>,

    id: PluginInstanceID,
}

pub(crate) struct PluginAudioThreadInstance {
    pub plugin: Box<dyn PluginAudioThread>,
    pub channel: Shared<PluginInstanceChannel>,
    pub last_process_status: ProcessStatus,
}

#[derive(Clone)]
pub(crate) struct SharedPluginAudioThreadInstance {
    shared: Shared<UnsafeCell<PluginAudioThreadInstance>>,
    id: PluginInstanceID,
}

impl SharedPluginAudioThreadInstance {
    fn new(
        plugin: Box<dyn PluginAudioThread>,
        id: PluginInstanceID,
        channel: Shared<PluginInstanceChannel>,
        coll_handle: &basedrop::Handle,
    ) -> Self {
        Self {
            shared: Shared::new(
                &coll_handle,
                UnsafeCell::new(PluginAudioThreadInstance {
                    plugin,
                    channel,
                    last_process_status: ProcessStatus::Continue,
                }),
            ),
            id,
        }
    }

    pub fn id(&self) -> &PluginInstanceID {
        &self.id
    }

    #[inline]
    pub unsafe fn borrow_mut(&self) -> &mut PluginAudioThreadInstance {
        &mut (*self.shared.get())
    }
}

struct PluginInstance {
    main_thread: Option<PluginMainThreadInstance>,
    audio_thread: Option<SharedPluginAudioThreadInstance>,
}

pub(crate) struct PluginInstancePool {
    plugin_instances: FnvHashMap<PluginInstanceID, PluginInstance>,
    id_accumulator: u64,

    coll_handle: basedrop::Handle,
}

impl PluginInstancePool {
    pub fn new(coll_handle: basedrop::Handle) -> Self {
        Self { plugin_instances: FnvHashMap::default(), id_accumulator: 0, coll_handle }
    }

    pub fn new_plugin_instance_from_main(
        &mut self,
        plugin: Box<dyn PluginMainThread>,
        plugin_type: PluginInstanceType,
        plugin_name: &'static str,
    ) -> PluginInstanceID {
        let id = PluginInstanceID { id: self.id_accumulator, plugin_type, name: plugin_name };
        self.id_accumulator += 1;

        let channel = Shared::new(
            &self.coll_handle,
            PluginInstanceChannel {
                restart_requested: AtomicBool::new(false),
                process_requested: AtomicBool::new(false),
                callback_requested: AtomicBool::new(false),
                active: AtomicBool::new(false),
            },
        );

        let main_thread = Some(PluginMainThreadInstance { plugin, channel, id });

        let _ =
            self.plugin_instances.insert(id, PluginInstance { main_thread, audio_thread: None });

        id
    }

    /// Only used for the special `Sum` and `DelayComp` types.
    pub fn new_plugin_instance_from_audio(
        &mut self,
        plugin: Box<dyn PluginAudioThread>,
        plugin_type: PluginInstanceType,
    ) -> PluginInstanceID {
        let id = PluginInstanceID { id: self.id_accumulator, plugin_type, name: "" };
        self.id_accumulator += 1;

        let channel = Shared::new(
            &self.coll_handle,
            PluginInstanceChannel {
                restart_requested: AtomicBool::new(false),
                process_requested: AtomicBool::new(false),
                callback_requested: AtomicBool::new(false),
                active: AtomicBool::new(true),
            },
        );

        let audio_thread =
            SharedPluginAudioThreadInstance::new(plugin, id, channel, &self.coll_handle);

        let _ = self
            .plugin_instances
            .insert(id, PluginInstance { main_thread: None, audio_thread: Some(audio_thread) });

        id
    }

    pub fn remove_plugin_instance(&mut self, id: PluginInstanceID, host: &mut Host) {
        // Deactivate the plugin first.
        self.deactivate_plugin_instance(id, host);

        if let Some(plugin_instance) = self.plugin_instances.remove(&id) {
            // Drop the plugin instance here.
            let _ = plugin_instance;
        }
    }

    // TODO: custom error type
    pub fn activate_plugin_instance(
        &mut self,
        id: PluginInstanceID,
        host: &mut Host,
        sample_rate: f64,
        min_frames: usize,
        max_frames: usize,
    ) -> Result<SharedPluginAudioThreadInstance, ()> {
        if let Some(plugin_instance) = self.plugin_instances.get_mut(&id) {
            if let Some(plugin_main_thread) = &mut plugin_instance.main_thread {
                if plugin_main_thread.channel.active.load(Ordering::Relaxed) {
                    // Cannot activate plugin that is already active.
                    return Err(());
                }

                // Prepare the host handle to accept requests from the plugin.
                host.current_plugin_channel = Shared::clone(&plugin_main_thread.channel);

                match plugin_main_thread.plugin.activate(
                    sample_rate,
                    min_frames,
                    max_frames,
                    host,
                    &self.coll_handle,
                ) {
                    Ok(plugin_audio_thread) => {
                        let new_audio_thread = SharedPluginAudioThreadInstance::new(
                            plugin_audio_thread,
                            plugin_main_thread.id,
                            Shared::clone(&plugin_main_thread.channel),
                            &self.coll_handle,
                        );

                        plugin_main_thread.channel.active.store(true, Ordering::Relaxed);

                        // Store the plugin audio thread instance for future schedules.
                        plugin_instance.audio_thread = Some(new_audio_thread.clone());

                        return Ok(new_audio_thread);
                    }
                    Err(e) => {
                        log::error!(
                            "Error while activating plugin instance {:?}: {}",
                            plugin_main_thread.id,
                            e
                        );
                        return Err(());
                    }
                }
            }
        }

        return Err(());
    }

    pub fn deactivate_plugin_instance(&mut self, id: PluginInstanceID, host: &mut Host) {
        if let Some(plugin_instance) = self.plugin_instances.get_mut(&id) {
            if let Some(plugin_main_thread) = &mut plugin_instance.main_thread {
                if !plugin_main_thread.channel.active.load(Ordering::Relaxed) {
                    // Plugin is already inactive.
                    return;
                }

                // Prepare the host handle to accept requests from the plugin.
                host.current_plugin_channel = Shared::clone(&plugin_main_thread.channel);

                plugin_main_thread.plugin.deactivate(host);

                plugin_main_thread.channel.active.store(false, Ordering::Relaxed);
            }

            if let Some(plugin_audio_thread) = plugin_instance.audio_thread.take() {
                // Drop the audio thread counterpart of the plugin here. (Note this will not
                // drop the actual instance until the schedule on the audio thread also drops
                // its pointer.)
                let _ = plugin_audio_thread;
            }
        }
    }
}
