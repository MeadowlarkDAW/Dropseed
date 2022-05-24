use std::error::Error;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use rusty_daw_core::SampleRate;

use crate::plugin::ext::audio_ports::AudioPortsExtension;
use crate::plugin::process_info::ProcBuffers;
use crate::plugin::{PluginAudioThread, PluginMainThread, PluginSaveState};
use crate::{HostRequest, ProcInfo, ProcessStatus};

use super::shared_pool::PluginInstanceID;

pub(crate) struct PluginInstanceHost {
    pub id: PluginInstanceID,

    main_thread: Option<Box<dyn PluginMainThread>>,

    state: Arc<SharedPluginState>,

    save_state: Option<PluginSaveState>,

    restart_requested: Arc<AtomicBool>,
    process_requested: Arc<AtomicBool>,
    callback_requested: Arc<AtomicBool>,
    deactivate_requested: Arc<AtomicBool>,
    remove_requested: bool,
}

impl PluginInstanceHost {
    pub fn new(
        id: PluginInstanceID,
        save_state: Option<PluginSaveState>,
        main_thread: Option<Box<dyn PluginMainThread>>,
        host_request: HostRequest,
    ) -> Self {
        let state = Arc::new(SharedPluginState::new());

        let restart_requested = Arc::clone(&host_request.restart_requested);
        let process_requested = Arc::clone(&host_request.process_requested);
        let callback_requested = Arc::clone(&host_request.callback_requested);

        let deactivate_requested = Arc::new(AtomicBool::new(false));

        if main_thread.is_none() {
            state.set(PluginState::InactiveWithError);
        }

        Self {
            id,
            main_thread,
            state: Arc::new(SharedPluginState::new()),
            save_state,
            restart_requested,
            process_requested,
            callback_requested,
            deactivate_requested,
            remove_requested: false,
        }
    }

    pub fn collect_save_state(&mut self) -> Option<PluginSaveState> {
        self.save_state.as_ref().map(|s| s.clone())
    }

    pub fn can_activate(&self) -> Result<(), ActivatePluginError> {
        if self.main_thread.is_none() {
            return Err(ActivatePluginError::NotLoaded);
        }
        if self.state.get().is_active() {
            return Err(ActivatePluginError::AlreadyActive);
        }
        if self.restart_requested.load(Ordering::Relaxed) {
            return Err(ActivatePluginError::RestartScheduled);
        }
        Ok(())
    }

    pub fn activate(
        &mut self,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
        coll_handle: &basedrop::Handle,
    ) -> Result<PluginInstanceHostAudioThread, ActivatePluginError> {
        self.can_activate()?;

        let plugin_main_thread = self.main_thread.as_mut().unwrap();

        if let Some(save_state) = &mut self.save_state {
            save_state.activation_requested = true;
        }

        match plugin_main_thread.activate(sample_rate, min_frames, max_frames, coll_handle) {
            Ok(plugin_audio_thread) => {
                self.process_requested.store(true, Ordering::Relaxed);
                self.deactivate_requested.store(false, Ordering::Relaxed);
                self.state.set(PluginState::ActiveAndSleeping);

                Ok(PluginInstanceHostAudioThread {
                    id: self.id.clone(),
                    plugin: plugin_audio_thread,
                    state: Arc::clone(&self.state),
                    process_requested: Arc::clone(&self.process_requested),
                    deactivate_requested: Arc::clone(&self.deactivate_requested),
                })
            }
            Err(e) => {
                self.state.set(PluginState::InactiveWithError);

                Err(ActivatePluginError::PluginSpecific(e))
            }
        }
    }

    pub fn schedule_deactivate(&mut self) {
        if let Some(save_state) = &mut self.save_state {
            save_state.activation_requested = false;
        }

        if !self.state.get().is_active() {
            return;
        }

        // Wait for the audio thread part to go to sleep before
        // deactivating.
        self.deactivate_requested.store(true, Ordering::Relaxed);
    }

    pub fn schedule_remove(&mut self) {
        self.remove_requested = true;

        self.schedule_deactivate();
    }

    pub fn audio_ports_ext(&mut self) -> Result<AudioPortsExtension, Box<dyn Error>> {
        if let Some(main_thread) = &mut self.main_thread {
            match main_thread.audio_ports_extension() {
                Ok(audio_ports_ext) => {
                    if let Some(save_state) = &mut self.save_state {
                        save_state.audio_in_out_channels = (
                            audio_ports_ext.total_in_channels() as u16,
                            audio_ports_ext.total_out_channels() as u16,
                        );
                    }

                    Ok(audio_ports_ext)
                }
                Err(e) => Err(e),
            }
        } else {
            Err("plugin is not loaded".into())
        }
    }

    pub fn on_idle(
        &mut self,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
        coll_handle: &basedrop::Handle,
    ) -> OnIdleResult {
        if self.main_thread.is_none() {
            if self.remove_requested {
                return OnIdleResult::PluginReadyToRemove;
            } else {
                return OnIdleResult::Ok;
            }
        }

        let plugin_main_thread = self.main_thread.as_mut().unwrap();

        if self.callback_requested.load(Ordering::Relaxed) {
            self.callback_requested.store(false, Ordering::Relaxed);

            plugin_main_thread.on_main_thread();
        }

        if self.restart_requested.load(Ordering::Relaxed) && !self.remove_requested {
            if self.state.get().is_active() {
                // Wait for the audio thread part to go to sleep before
                // deactivating.
                self.deactivate_requested.store(true, Ordering::Relaxed);
            } else if self.restart_requested.load(Ordering::Relaxed) {
                self.restart_requested.store(false, Ordering::Relaxed);

                match self.activate(sample_rate, min_frames, max_frames, coll_handle) {
                    Ok(_) => return OnIdleResult::PluginRestarted,
                    Err(e) => return OnIdleResult::PluginFailedToRestart(e),
                }
            }
        }

        if self.deactivate_requested.load(Ordering::Relaxed) {
            if self.state.get() == PluginState::ActiveAndReadyToDeactivate {
                // Safe to deactive now.

                plugin_main_thread.deactivate();

                self.state.set(PluginState::Inactive);
                self.deactivate_requested.store(false, Ordering::Relaxed);

                if !self.remove_requested {
                    let mut res = OnIdleResult::PluginDeactivated;

                    if self.restart_requested.load(Ordering::Relaxed) {
                        self.restart_requested.store(false, Ordering::Relaxed);

                        match self.activate(sample_rate, min_frames, max_frames, coll_handle) {
                            Ok(_) => res = OnIdleResult::PluginRestarted,
                            Err(e) => res = OnIdleResult::PluginFailedToRestart(e),
                        }
                    }

                    return res;
                }
            }
        }

        if self.remove_requested {
            if !self.state.get().is_active() {
                return OnIdleResult::PluginReadyToRemove;
            }
        }

        OnIdleResult::Ok
    }
}

pub enum OnIdleResult {
    Ok,
    PluginDeactivated,
    PluginRestarted,
    PluginReadyToRemove,
    PluginFailedToRestart(ActivatePluginError),
}

pub(crate) struct PluginInstanceHostAudioThread {
    pub id: PluginInstanceID,

    plugin: Box<dyn PluginAudioThread>,

    state: Arc<SharedPluginState>,

    process_requested: Arc<AtomicBool>,
    deactivate_requested: Arc<AtomicBool>,
}

impl PluginInstanceHostAudioThread {
    pub fn process<'a>(&mut self, proc_info: &ProcInfo, buffers: &mut ProcBuffers) {
        let clear_outputs = |proc_info: &ProcInfo, buffers: &mut ProcBuffers| {
            // Safe because this `proc_info.frames` will always be less than or
            // equal to the length of all audio buffers.
            unsafe {
                buffers.clear_all_outputs_unchecked(proc_info.frames);
            }
        };

        let state = self.state.get();

        if !state.is_active() {
            // Can't process a plugin that is not active.
            clear_outputs(proc_info, buffers);
            return;
        }

        // Do we want to deactivate the plugin?
        if self.deactivate_requested.load(Ordering::Relaxed) {
            if state.is_processing() {
                self.plugin.stop_processing();
            }

            self.state.set(PluginState::ActiveAndReadyToDeactivate);
            clear_outputs(proc_info, buffers);
            return;
        }

        if state == PluginState::ActiveWithError {
            // We can't process a plugin which failed to start processing.
            clear_outputs(proc_info, buffers);
            return;
        }

        if state == PluginState::ActiveAndWaitingForQuiet {
            if buffers.audio_inputs_silent(proc_info.frames) {
                self.plugin.stop_processing();

                self.state.set(PluginState::ActiveAndSleeping);
                clear_outputs(proc_info, buffers);
                return;
            }
        }

        // TODO: Handle input events

        if state.is_sleeping() {
            let has_in_events = true; // TODO: Check if there are any input events.

            if !self.process_requested.load(Ordering::Relaxed) && !has_in_events {
                // The plugin is sleeping, there is no request to wake it up, and there
                // are no events to process.
                clear_outputs(proc_info, buffers);
                return;
            }

            self.process_requested.store(false, Ordering::Relaxed);

            if let Err(_) = self.plugin.start_processing() {
                // The plugin failed to start processing.
                self.state.set(PluginState::ActiveWithError);
                clear_outputs(proc_info, buffers);
                return;
            }

            self.state.set(PluginState::ActiveAndProcessing);
        }

        let mut status = ProcessStatus::Sleep;

        if self.state.get().is_processing() {
            status = self.plugin.process(proc_info, buffers);
        }

        // TODO: Handle output events

        match status {
            ProcessStatus::Continue => {
                self.state.set(PluginState::ActiveAndProcessing);
            }
            ProcessStatus::ContinueIfNotQuiet => {
                self.state.set(PluginState::ActiveAndWaitingForQuiet);
            }
            ProcessStatus::Tail => {
                self.state.set(PluginState::ActiveAndWaitingForTail);
            }
            ProcessStatus::Sleep => {
                if self.state.get().is_processing() {
                    self.plugin.stop_processing();

                    // Do we want to deactivate the plugin?
                    if self.deactivate_requested.load(Ordering::Relaxed) {
                        self.state.set(PluginState::ActiveAndReadyToDeactivate);
                    } else {
                        self.state.set(PluginState::ActiveAndSleeping);
                    }

                    return;
                }
            }
            ProcessStatus::Error => {
                // Discard all output buffers.
                clear_outputs(proc_info, buffers);
            }
        }

        if self.state.get() == PluginState::ActiveAndWaitingForTail {
            if buffers.audio_outputs_silent(proc_info.frames) {
                self.plugin.stop_processing();

                // Do we want to deactivate the plugin?
                if self.deactivate_requested.load(Ordering::Relaxed) {
                    self.state.set(PluginState::ActiveAndReadyToDeactivate);
                } else {
                    self.state.set(PluginState::ActiveAndSleeping);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum PluginState {
    /// The plugin is inactive, only the main thread uses it
    Inactive = 0,

    /// Activation failed
    InactiveWithError = 1,

    /// The plugin is active and sleeping, the audio engine can call start_processing()
    ActiveAndSleeping = 2,

    /// The plugin is processing
    ActiveAndProcessing = 3,

    /// The plugin is processing, but will be put to sleep the next time all input buffers
    /// are silent.
    ActiveAndWaitingForQuiet = 4,

    /// The plugin is processing, but will be put to sleep at the end of the plugin's tail.
    ActiveAndWaitingForTail = 5,

    /// The plugin did process but is in error
    ActiveWithError = 6,

    /// The plugin is not used anymore by the audio engine and can be deactivated on the main
    /// thread
    ActiveAndReadyToDeactivate = 7,
}

impl PluginState {
    pub fn is_active(&self) -> bool {
        match self {
            PluginState::Inactive | PluginState::InactiveWithError => false,
            _ => true,
        }
    }

    pub fn is_processing(&self) -> bool {
        match self {
            PluginState::ActiveAndProcessing
            | PluginState::ActiveAndWaitingForQuiet
            | PluginState::ActiveAndWaitingForTail => true,
            _ => false,
        }
    }

    pub fn is_sleeping(&self) -> bool {
        *self == PluginState::ActiveAndSleeping
    }
}

#[derive(Debug)]
pub(crate) struct SharedPluginState(AtomicU32);

impl SharedPluginState {
    pub fn new() -> Self {
        Self(AtomicU32::new(0))
    }

    #[inline]
    pub fn get(&self) -> PluginState {
        let s = self.0.load(Ordering::Relaxed);

        // Safe because we set `#[repr(u32)]` on this enum, and this AtomicU32
        // can never be set to a value that is out of range.
        unsafe { *(&s as *const u32 as *const PluginState) }
    }

    #[inline]
    pub fn set(&self, state: PluginState) {
        // Safe because we set `#[repr(u32)]` on this enum.
        let s = unsafe { *(&state as *const PluginState as *const u32) };

        self.0.store(s, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub enum ActivatePluginError {
    NotLoaded,
    AlreadyActive,
    RestartScheduled,
    PluginFailedToGetAudioPortsExt(Box<dyn Error>),
    PluginSpecific(Box<dyn Error>),
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
            ActivatePluginError::PluginSpecific(e) => {
                write!(f, "plugin returned error while activating: {:?}", e)
            }
        }
    }
}

impl From<Box<dyn Error>> for ActivatePluginError {
    fn from(e: Box<dyn Error>) -> Self {
        ActivatePluginError::PluginSpecific(e)
    }
}
