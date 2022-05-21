use basedrop::Shared;
use std::cell::UnsafeCell;
use std::error::Error;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use rusty_daw_core::SampleRate;

use crate::plugin::{PluginAudioThread, PluginMainThread};
use crate::{ProcInfo, ProcessStatus};

pub(crate) struct PluginInstanceHost {
    main_thread: Option<Box<dyn PluginMainThread>>,

    audio_thread: Option<PluginInstanceHostAudioThread>,

    state: Arc<SharedPluginState>,

    restart_requested: Arc<AtomicBool>,
    process_requested: Arc<AtomicBool>,
    callback_requested: Arc<AtomicBool>,
    deactivate_requested: Arc<AtomicBool>,
}

impl PluginInstanceHost {
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
    ) -> Result<(), ActivatePluginError> {
        self.can_activate()?;

        let plugin_main_thread = self.main_thread.as_mut().unwrap();

        match plugin_main_thread.activate(sample_rate, min_frames, max_frames, coll_handle) {
            Ok(plugin_audio_thread) => {
                self.audio_thread = Some(PluginInstanceHostAudioThread {
                    plugin: Shared::new(coll_handle, UnsafeCell::new(plugin_audio_thread)),
                    state: Arc::clone(&self.state),
                });

                self.process_requested.store(true, Ordering::Relaxed);
                self.deactivate_requested.store(false, Ordering::Relaxed);
                self.state.set(PluginState::ActiveAndSleeping);

                Ok(())
            }
            Err(e) => {
                self.state.set(PluginState::InactiveWithError);

                Err(ActivatePluginError::PluginSpecific(e))
            }
        }
    }

    pub fn deactivate(&mut self) {
        let state = self.state.get();

        if !state.is_active() {
            return;
        }

        if !(state.is_processing() || state.is_sleeping()) {
            // Safe to deactive right now.

            // This is always `Some` when `state.is_active() == true`.
            let plugin_main_thread = self.main_thread.as_mut().unwrap();

            plugin_main_thread.deactivate();

            self.state.set(PluginState::Inactive);
        }
        {
            // Else wait for the audio thread part to go to sleep before
            // deactivating.
            self.deactivate_requested.store(true, Ordering::Relaxed);
        }
    }
}

pub(crate) struct PluginInstanceHostAudioThread {
    plugin: Shared<UnsafeCell<Box<dyn PluginAudioThread>>>,

    state: Arc<SharedPluginState>,

    process_requested: Arc<AtomicBool>,
    deactivate_requested: Arc<AtomicBool>,
}

impl PluginInstanceHostAudioThread {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        let state = self.state.get();

        if !state.is_active() {
            // Can't process a plugin that is not active.
            proc_info.clear_all_outputs();
            return;
        }

        let plugin = unsafe { &mut *self.plugin.get() };

        // Do we want to deactivate the plugin?
        if self.deactivate_requested.load(Ordering::Relaxed) {
            if state.is_processing() {
                plugin.stop_processing();
            }

            self.state.set(PluginState::ActiveAndReadyToDeactivate);
            proc_info.clear_all_outputs();
            return;
        }

        if state == PluginState::ActiveWithError {
            // We can't process a plugin which failed to start processing.
            proc_info.clear_all_outputs();
            return;
        }

        if state == PluginState::ActiveAndWaitingForQuiet {
            if proc_info.audio_inputs_silent() {
                plugin.stop_processing();

                self.state.set(PluginState::ActiveAndSleeping);
                proc_info.clear_all_outputs();
                return;
            }
        }

        // TODO: Handle input events

        if state.is_sleeping() {
            let has_in_events = true; // TODO

            if !self.process_requested.load(Ordering::Relaxed) && !has_in_events {
                // The plugin is sleeping, there is no request to wake it up, and there
                // are no events to process.
                proc_info.clear_all_outputs();
                return;
            }

            self.process_requested.store(false, Ordering::Relaxed);

            if let Err(_) = plugin.start_processing() {
                // The plugin failed to start processing.
                self.state.set(PluginState::ActiveWithError);
                proc_info.clear_all_outputs();
                return;
            }

            self.state.set(PluginState::ActiveAndProcessing);
        }

        let mut status = ProcessStatus::Sleep;

        if self.state.get().is_processing() {
            status = plugin.process(proc_info);
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
                    plugin.stop_processing();

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
                proc_info.clear_all_outputs();
            }
        }

        if self.state.get() == PluginState::ActiveAndWaitingForTail {
            if proc_info.audio_outputs_silent() {
                plugin.stop_processing();

                self.state.set(PluginState::ActiveAndSleeping);
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
            ActivatePluginError::PluginSpecific(e) => write!(f, "plugin returned error: {:?}", e),
        }
    }
}

impl From<Box<dyn Error>> for ActivatePluginError {
    fn from(e: Box<dyn Error>) -> Self {
        ActivatePluginError::PluginSpecific(e)
    }
}
