use fnv::FnvHashMap;
use rtrb_basedrop::{Consumer, Producer, RingBuffer};
use rusty_daw_core::SampleRate;
use std::error::Error;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use crate::host_request::RequestFlags;
use crate::plugin::event_queue::EventQueue;
use crate::plugin::events::{
    EventFlags, EventParamGesture, EventParamMod, EventParamValue, PluginEvent,
};
use crate::plugin::ext::audio_ports::PluginAudioPortsExt;
use crate::plugin::process_info::ProcBuffers;
use crate::plugin::{PluginAudioThread, PluginMainThread, PluginSaveState};
use crate::{HostRequest, ProcInfo, ProcessStatus};

use super::shared_pool::PluginInstanceID;

#[derive(Clone, Copy)]
struct MainToAudioParamMsg {
    param: u32,
    value: f64,
}

#[derive(Clone, Copy)]
enum AudioToMainParamMsgType {
    Value(f64),
    HasGesture,
    IsBegin,
    IsNotBegin,
}

#[derive(Clone, Copy)]
struct AudioToMainParamMsg {
    param: u32,
    msg_type: AudioToMainParamMsgType,
}

struct ParamQueuesMainThread {
    main_to_audio_param_value_tx: Producer<MainToAudioParamMsg>,
    main_to_audio_param_mod_tx: Producer<MainToAudioParamMsg>,

    audio_to_main_param_value_rx: Consumer<AudioToMainParamMsg>,
}

struct ParamQueuesAudioThread {
    audio_to_main_param_value_tx: Producer<AudioToMainParamMsg>,

    main_to_audio_param_value_rx: Consumer<MainToAudioParamMsg>,
    main_to_audio_param_mod_rx: Consumer<MainToAudioParamMsg>,
}

pub(crate) struct PluginInstanceHost {
    pub id: PluginInstanceID,

    pub audio_ports_ext: Option<PluginAudioPortsExt>,

    main_thread: Option<Box<dyn PluginMainThread>>,

    state: Arc<SharedPluginState>,

    save_state: Option<PluginSaveState>,

    param_queues: Option<ParamQueuesMainThread>,

    host_request: HostRequest,
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

        if main_thread.is_none() {
            state.set(PluginState::InactiveWithError);
        }

        Self {
            id,
            main_thread,
            audio_ports_ext: None,
            state: Arc::new(SharedPluginState::new()),
            save_state,
            param_queues: None,
            host_request,
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
        if self.host_request.load_requested().contains(RequestFlags::RESTART) {
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
    ) -> Result<(PluginInstanceHostAudioThread, PluginAudioPortsExt), ActivatePluginError> {
        self.can_activate()?;

        let plugin_main_thread = self.main_thread.as_mut().unwrap();

        if let Some(save_state) = &mut self.save_state {
            save_state.activation_requested = true;
        }

        let audio_ports = match plugin_main_thread.audio_ports_ext() {
            Ok(audio_ports) => audio_ports.clone(),
            Err(e) => {
                self.state.set(PluginState::InactiveWithError);

                return Err(ActivatePluginError::PluginFailedToGetAudioPortsExt(e));
            }
        };

        self.audio_ports_ext = Some(audio_ports.clone());
        if let Some(save_state) = &mut self.save_state {
            save_state.audio_in_out_channels =
                (audio_ports.total_in_channels() as u16, audio_ports.total_out_channels() as u16);
        }

        match plugin_main_thread.activate(sample_rate, min_frames, max_frames, coll_handle) {
            Ok(plugin_audio_thread) => {
                self.host_request.reset_deactivate();
                self.host_request.request_process();

                self.state.set(PluginState::ActiveAndSleeping);

                let num_params = 5; // TODO

                let (param_queues_main_thread, param_queues_audio_thread) = if num_params > 0 {
                    // TODO: Tweak these capacities?
                    let (main_to_audio_param_value_tx, main_to_audio_param_value_rx) =
                        RingBuffer::new(num_params * 3, coll_handle);
                    let (main_to_audio_param_mod_tx, main_to_audio_param_mod_rx) =
                        RingBuffer::new(num_params * 2, coll_handle);
                    let (audio_to_main_param_value_tx, audio_to_main_param_value_rx) =
                        RingBuffer::new(num_params * 3, coll_handle);

                    (
                        Some(ParamQueuesMainThread {
                            main_to_audio_param_value_tx,
                            main_to_audio_param_mod_tx,
                            audio_to_main_param_value_rx,
                        }),
                        Some(ParamQueuesAudioThread {
                            audio_to_main_param_value_tx,
                            main_to_audio_param_value_rx,
                            main_to_audio_param_mod_rx,
                        }),
                    )
                } else {
                    (None, None)
                };

                self.param_queues = param_queues_main_thread;

                let mut in_param_event_reducer = FnvHashMap::default();
                in_param_event_reducer.reserve(num_params);

                Ok((
                    PluginInstanceHostAudioThread {
                        id: self.id.clone(),
                        plugin: plugin_audio_thread,
                        state: Arc::clone(&self.state),
                        param_queues: param_queues_audio_thread,
                        in_events: EventQueue::new(num_params * 3),
                        out_events: EventQueue::new(num_params * 3),
                        in_param_event_reducer,
                        host_request: self.host_request.clone(),
                    },
                    audio_ports,
                ))
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
        self.host_request.request_deactivate();
    }

    pub fn schedule_remove(&mut self) {
        self.remove_requested = true;

        self.schedule_deactivate();
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

        let request_flags = self.host_request.load_requests_and_reset_callback();
        let state = self.state.get();

        if self.remove_requested {
            if !state.is_active() {
                return OnIdleResult::PluginReadyToRemove;
            }
        }

        if request_flags.contains(RequestFlags::CALLBACK) {
            plugin_main_thread.on_main_thread();
        }

        if request_flags.contains(RequestFlags::DEACTIVATE) {
            if state == PluginState::ActiveAndReadyToDeactivate {
                // Safe to deactive now.

                plugin_main_thread.deactivate();

                self.state.set(PluginState::Inactive);
                self.host_request.reset_deactivate();

                if !self.remove_requested {
                    let mut res = OnIdleResult::PluginDeactivated;

                    if self.host_request.reset_restart() {
                        match self.activate(sample_rate, min_frames, max_frames, coll_handle) {
                            Ok((audio_thread, audio_ports)) => {
                                res = OnIdleResult::PluginActivated(audio_thread, audio_ports)
                            }
                            Err(e) => res = OnIdleResult::PluginFailedToActivate(e),
                        }
                    }

                    return res;
                }
            }
        } else if request_flags.contains(RequestFlags::RESTART) && !self.remove_requested {
            // Wait for the audio thread part to go to sleep before
            // deactivating.
            self.host_request.request_deactivate();
        }

        OnIdleResult::Ok
    }
}

pub(crate) enum OnIdleResult {
    Ok,
    PluginDeactivated,
    PluginActivated(PluginInstanceHostAudioThread, PluginAudioPortsExt),
    PluginReadyToRemove,
    PluginFailedToActivate(ActivatePluginError),
}

pub(crate) struct PluginInstanceHostAudioThread {
    pub id: PluginInstanceID,

    plugin: Box<dyn PluginAudioThread>,

    state: Arc<SharedPluginState>,

    param_queues: Option<ParamQueuesAudioThread>,
    in_events: EventQueue,
    out_events: EventQueue,
    in_param_event_reducer: FnvHashMap<u32, MainToAudioParamMsg>,

    host_request: HostRequest,
}

impl PluginInstanceHostAudioThread {
    pub fn process<'a>(&mut self, proc_info: &ProcInfo, buffers: &mut ProcBuffers) {
        // TODO: Flush parameters while plugin is sleeping.

        let clear_outputs = |proc_info: &ProcInfo, buffers: &mut ProcBuffers| {
            // Safe because this `proc_info.frames` will always be less than or
            // equal to the length of all audio buffers.
            unsafe {
                buffers.clear_all_outputs_unchecked(proc_info.frames);
            }
            for b in buffers.audio_out.iter_mut() {
                b.sync_constant_mask_to_buffers();
            }
        };

        let mut state = self.state.get();

        if !state.is_active() {
            // Can't process a plugin that is not active.
            clear_outputs(proc_info, buffers);
            self.in_events.clear();
            return;
        }

        let request_flags = self.host_request.load_requested();

        // Do we want to deactivate the plugin?
        if request_flags.contains(RequestFlags::DEACTIVATE) {
            if state.is_processing() {
                self.plugin.stop_processing();
            }

            self.state.set(PluginState::ActiveAndReadyToDeactivate);
            clear_outputs(proc_info, buffers);
            self.in_events.clear();
            return;
        }

        if state == PluginState::ActiveWithError {
            // We can't process a plugin which failed to start processing.
            clear_outputs(proc_info, buffers);
            self.in_events.clear();
            return;
        }

        self.out_events.clear();

        if let Some(params_queue) = &mut self.param_queues {
            self.in_param_event_reducer.clear();

            while let Ok(param_event) = params_queue.main_to_audio_param_value_rx.pop() {
                let _ = self.in_param_event_reducer.insert(param_event.param, param_event);
            }
            for (_, param_event) in self.in_param_event_reducer.drain() {
                self.in_events.push(PluginEvent::ParamValue(&EventParamValue::new(
                    // TODO: Finer values for `time` instead of just setting it to the first frame?
                    0,                   // time
                    0,                   // space_id
                    EventFlags::empty(), // event_flags
                    param_event.param,   // param_id
                    // TODO: Note ID
                    -1, // note_id
                    // TODO: Port index
                    -1, // port_index
                    // TODO: Channel
                    -1, // channel
                    // TODO: Key
                    -1,                // key
                    param_event.value, // value
                )))
            }

            self.in_param_event_reducer.clear();

            while let Ok(param_event) = params_queue.main_to_audio_param_mod_rx.pop() {
                let _ = self.in_param_event_reducer.insert(param_event.param, param_event);
            }
            for (_, param_event) in self.in_param_event_reducer.drain() {
                self.in_events.push(PluginEvent::ParamMod(&EventParamMod::new(
                    // TODO: Finer values for `time` instead of just setting it to the first frame?
                    0,                   // time
                    0,                   // space_id
                    EventFlags::empty(), // event_flags
                    param_event.param,   // param_id
                    // TODO: Note ID
                    -1, // note_id
                    // TODO: Port index
                    -1, // port_index
                    // TODO: Channel
                    -1, // channel
                    // TODO: Key
                    -1,                // key
                    param_event.value, // amount
                )))
            }
        }

        if state == PluginState::ActiveAndWaitingForQuiet {
            // Sync constant masks for more efficient silence checking.
            for buf in buffers.audio_in.iter_mut() {
                buf.sync_constant_mask_from_buffers();
            }

            if buffers.audio_inputs_silent(proc_info.frames) {
                self.plugin.stop_processing();

                self.state.set(PluginState::ActiveAndSleeping);
                clear_outputs(proc_info, buffers);
                self.in_events.clear();
                return;
            }
        }

        if state.is_sleeping() {
            let has_in_events = true; // TODO: Check if there are any input events.

            if !request_flags.contains(RequestFlags::PROCESS) && !has_in_events {
                // The plugin is sleeping, there is no request to wake it up, and there
                // are no events to process.
                clear_outputs(proc_info, buffers);
                self.in_events.clear();
                return;
            }

            self.host_request.reset_process();

            if let Err(_) = self.plugin.start_processing() {
                // The plugin failed to start processing.
                self.state.set(PluginState::ActiveWithError);
                clear_outputs(proc_info, buffers);
                self.in_events.clear();
                return;
            }

            self.state.set(PluginState::ActiveAndProcessing);
            state = PluginState::ActiveAndProcessing;
        }

        // Sync constant masks for the plugin.
        if state != PluginState::ActiveAndWaitingForQuiet {
            for buf in buffers.audio_in.iter_mut() {
                buf.sync_constant_mask_from_buffers();
            }
        }
        for buf in buffers.audio_out.iter_mut() {
            buf.set_constant_mask(0);
        }

        let status = self.plugin.process(proc_info, buffers, &self.in_events, &mut self.out_events);

        self.in_events.clear();

        while let Some(out_event) = self.out_events.pop() {
            match out_event.get() {
                Ok(PluginEvent::ParamGesture(event)) => {
                    // TODO
                }
                Ok(PluginEvent::ParamValue(event)) => {
                    // TODO
                }
                // TODO: Handle more output event types
                _ => {}
            }
        }

        match status {
            ProcessStatus::Continue => {
                if state != PluginState::ActiveAndProcessing {
                    self.state.set(PluginState::ActiveAndProcessing);
                }
            }
            ProcessStatus::ContinueIfNotQuiet => {
                if state != PluginState::ActiveAndWaitingForQuiet {
                    self.state.set(PluginState::ActiveAndWaitingForQuiet);
                }
            }
            ProcessStatus::Tail => {
                if state != PluginState::ActiveAndProcessing {
                    self.state.set(PluginState::ActiveAndProcessing);
                }

                if buffers.audio_outputs_silent(proc_info.frames) {
                    self.plugin.stop_processing();

                    self.state.set(PluginState::ActiveAndSleeping);
                }
            }
            ProcessStatus::Sleep => {
                self.plugin.stop_processing();

                self.state.set(PluginState::ActiveAndSleeping);
            }
            ProcessStatus::Error => {
                // Discard all output buffers.
                clear_outputs(proc_info, buffers);
                return;
            }
        }

        for buf in buffers.audio_out.iter_mut() {
            buf.sync_constant_mask_to_buffers();
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

    /// The plugin did process but is in error
    ActiveWithError = 5,

    /// The plugin is not used anymore by the audio engine and can be deactivated on the main
    /// thread
    ActiveAndReadyToDeactivate = 6,
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
            PluginState::ActiveAndProcessing | PluginState::ActiveAndWaitingForQuiet => true,
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
        // TODO: Are we able to use relaxed ordering here?
        let s = self.0.load(Ordering::SeqCst);

        // Safe because we set `#[repr(u32)]` on this enum, and this AtomicU32
        // can never be set to a value that is out of range.
        unsafe { *(&s as *const u32 as *const PluginState) }
    }

    #[inline]
    pub fn set(&self, state: PluginState) {
        // Safe because we set `#[repr(u32)]` on this enum.
        let s = unsafe { *(&state as *const PluginState as *const u32) };

        // TODO: Are we able to use relaxed ordering here?
        self.0.store(s, Ordering::SeqCst);
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
