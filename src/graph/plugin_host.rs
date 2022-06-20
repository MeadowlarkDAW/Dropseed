use basedrop::Shared;
use fnv::FnvHashMap;
use meadowlark_core_types::SampleRate;
use smallvec::SmallVec;
use std::error::Error;
use std::fmt::Debug;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use super::shared_pool::PluginInstanceID;
use crate::engine::plugin_scanner::PluginFormat;
use crate::graph::shared_pool::SharedBuffer;
use crate::plugin::events::event_queue::{EventQueue, ProcEventRefMut};
use crate::plugin::events::{EventFlags, EventParamMod, EventParamValue};
use crate::plugin::ext::audio_ports::PluginAudioPortsExt;
use crate::plugin::ext::note_ports::PluginNotePortsExt;
use crate::plugin::ext::params::{ParamInfo, ParamInfoFlags};
use crate::plugin::host_request::RequestFlags;
use crate::plugin::process_info::ProcBuffers;
use crate::plugin::{PluginAudioThread, PluginMainThread, PluginPreset, PluginSaveState};
use crate::utils::reducing_queue::{
    ReducFnvConsumer, ReducFnvProducer, ReducFnvValue, ReducingFnvQueue,
};
use crate::{HostRequest, ParamID, ProcEvent, ProcInfo, ProcessStatus, ScannedPluginKey};

#[derive(Clone, Copy)]
struct MainToAudioParamValue {
    value: f64,
    _cookie: *const std::ffi::c_void,
}

unsafe impl Sync for MainToAudioParamValue {}
unsafe impl Send for MainToAudioParamValue {}

impl ReducFnvValue for MainToAudioParamValue {}

#[derive(Debug, Clone, Copy)]
pub struct ParamGestureInfo {
    pub is_begin: bool,
}

#[derive(Clone, Copy)]
struct AudioToMainParamValue {
    value: Option<f64>,
    gesture: Option<ParamGestureInfo>,
}

impl ReducFnvValue for AudioToMainParamValue {
    fn update(&mut self, new_value: &Self) {
        if new_value.value.is_some() {
            self.value = new_value.value;
        }

        if new_value.gesture.is_some() {
            self.gesture = new_value.gesture;
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParamModifiedInfo {
    pub param_id: ParamID,
    pub new_value: Option<f64>,
    pub is_gesturing: bool,
}

#[derive(Debug)]
pub struct PluginHandle {
    pub params: PluginParamsExt,
    pub(crate) audio_ports: PluginAudioPortsExt,
    pub(crate) note_ports: PluginNotePortsExt,
}

impl PluginHandle {
    pub fn audio_ports(&self) -> &PluginAudioPortsExt {
        &self.audio_ports
    }

    pub fn note_ports(&self) -> &PluginNotePortsExt {
        &self.note_ports
    }
}

pub struct PluginParamsExt {
    /// (parameter info, initial value)
    pub params: FnvHashMap<ParamID, ParamInfo>,

    ui_to_audio_param_value_tx: Option<ReducFnvProducer<ParamID, MainToAudioParamValue>>,
    ui_to_audio_param_mod_tx: Option<ReducFnvProducer<ParamID, MainToAudioParamValue>>,
}

impl Debug for PluginParamsExt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("PluginParamsExt");
        f.field("params", &self.params);
        f.finish()
    }
}

impl PluginParamsExt {
    pub fn set_value(&mut self, param_id: ParamID, value: f64) {
        if let Some(ui_to_audio_param_value_tx) = &mut self.ui_to_audio_param_value_tx {
            if let Some(param_info) = self.params.get(&param_id) {
                if param_info.flags.contains(ParamInfoFlags::IS_READONLY) {
                    log::warn!("Ignored request to set parameter value: parameter with id {:?} is read only", &param_id);
                } else {
                    ui_to_audio_param_value_tx
                        .set(param_id, MainToAudioParamValue { value, _cookie: param_info.cookie });
                    ui_to_audio_param_value_tx.producer_done();
                }
            } else {
                log::warn!(
                    "Ignored request to set parameter value: plugin has no parameter with id {:?}",
                    &param_id
                );
            }
        } else {
            log::warn!("Ignored request to set parameter value: plugin has no parameters");
        }
    }

    pub fn set_mod_amount(&mut self, param_id: ParamID, amount: f64) {
        if let Some(ui_to_audio_param_mod_tx) = &mut self.ui_to_audio_param_mod_tx {
            if let Some(param_info) = self.params.get(&param_id) {
                ui_to_audio_param_mod_tx.set(
                    param_id,
                    MainToAudioParamValue { value: amount, _cookie: param_info.cookie },
                );
                ui_to_audio_param_mod_tx.producer_done();
            } else {
                log::warn!(
                    "Ignored request to set parameter mod amount: plugin has no parameter with id {:?}",
                    &param_id
                );
            }
        } else {
            log::warn!("Ignored request to set parameter mod amount: plugin has no parameters");
        }
    }
}

struct ParamQueuesMainThread {
    audio_to_main_param_value_rx: ReducFnvConsumer<ParamID, AudioToMainParamValue>,
}

struct ParamQueuesAudioThread {
    audio_to_main_param_value_tx: ReducFnvProducer<ParamID, AudioToMainParamValue>,

    ui_to_audio_param_value_rx: ReducFnvConsumer<ParamID, MainToAudioParamValue>,
    ui_to_audio_param_mod_rx: ReducFnvConsumer<ParamID, MainToAudioParamValue>,
}

pub(crate) struct PluginInstanceHost {
    pub id: PluginInstanceID,

    pub audio_ports: Option<PluginAudioPortsExt>,
    pub note_ports: Option<PluginNotePortsExt>,

    pub num_audio_in_channels: usize,
    pub num_audio_out_channels: usize,

    main_thread: Option<Box<dyn PluginMainThread>>,

    state: Arc<SharedPluginState>,

    save_state: PluginSaveState,
    plugin_version: Option<Shared<String>>,

    param_queues: Option<ParamQueuesMainThread>,
    gesturing_params: FnvHashMap<ParamID, bool>,

    host_request: HostRequest,
    remove_requested: bool,
}

impl PluginInstanceHost {
    pub fn new(
        id: PluginInstanceID,
        save_state: PluginSaveState,
        mut main_thread: Option<Box<dyn PluginMainThread>>,
        host_request: HostRequest,
        plugin_version: Option<Shared<String>>,
    ) -> Self {
        let state = Arc::new(SharedPluginState::new());

        if let Some(preset) = &save_state.preset {
            if let Some(main_thread) = &mut main_thread {
                match main_thread.load_state(preset) {
                    Ok(()) => {
                        log::trace!("Plugin {:?} successfully loaded preset", &id);
                    }
                    Err(e) => {
                        log::error!("Plugin {:?} failed to load preset: {}", &id, e);
                    }
                }
            }
        }

        if main_thread.is_none() {
            state.set(PluginState::InactiveWithError);
        }

        let (num_audio_in_channels, num_audio_out_channels) =
            if let Some(backup_audio_ports) = &save_state.backup_audio_ports {
                (backup_audio_ports.total_in_channels(), backup_audio_ports.total_out_channels())
            } else {
                (0, 0)
            };

        Self {
            id,
            main_thread,
            audio_ports: None,
            note_ports: None,
            num_audio_in_channels,
            num_audio_out_channels,
            state: Arc::new(SharedPluginState::new()),
            save_state,
            plugin_version,
            param_queues: None,
            gesturing_params: FnvHashMap::default(),
            host_request,
            remove_requested: false,
        }
    }

    pub fn new_graph_in(
        id: PluginInstanceID,
        host_request: HostRequest,
        num_audio_out_channels: usize,
    ) -> Self {
        let state = Arc::new(SharedPluginState::new());

        state.set(PluginState::Inactive);

        // We don't actually use this save state. This is just here to be
        // consistent with the rest of the plugins.
        let save_state = PluginSaveState {
            key: ScannedPluginKey {
                rdn: "app.meadowlark.dropseed-graph-in".into(),
                format: PluginFormat::Internal,
            },
            activation_requested: false,
            backup_audio_ports: None,
            backup_note_ports: None,
            preset: None,
        };

        Self {
            id,
            main_thread: None,
            audio_ports: None,
            note_ports: None,
            num_audio_in_channels: 0,
            num_audio_out_channels,
            state: Arc::new(SharedPluginState::new()),
            save_state,
            plugin_version: None,
            param_queues: None,
            gesturing_params: FnvHashMap::default(),
            host_request,
            remove_requested: false,
        }
    }

    pub fn new_graph_out(
        id: PluginInstanceID,
        host_request: HostRequest,
        num_audio_in_channels: usize,
    ) -> Self {
        let state = Arc::new(SharedPluginState::new());

        state.set(PluginState::Inactive);

        // We don't actually use this save state. This is just here to be
        // consistent with the rest of the plugins.
        let save_state = PluginSaveState {
            key: ScannedPluginKey {
                rdn: "app.meadowlark.dropseed-graph-out".into(),
                format: PluginFormat::Internal,
            },
            activation_requested: false,
            backup_audio_ports: None,
            backup_note_ports: None,
            preset: None,
        };

        Self {
            id,
            main_thread: None,
            audio_ports: None,
            note_ports: None,
            num_audio_in_channels,
            num_audio_out_channels: 0,
            state: Arc::new(SharedPluginState::new()),
            save_state,
            plugin_version: None,
            param_queues: None,
            gesturing_params: FnvHashMap::default(),
            host_request,
            remove_requested: false,
        }
    }

    pub fn collect_save_state(&mut self) -> PluginSaveState {
        if self.host_request.state_marked_dirty_and_reset_dirty() {
            if let Some(main_thread) = &mut self.main_thread {
                let preset = match main_thread.collect_save_state() {
                    Ok(preset) => preset.map(|p| PluginPreset {
                        version: self.plugin_version.as_ref().map(|v| String::clone(&*v)),
                        data: p,
                    }),
                    Err(e) => {
                        log::error!(
                            "Failed to collect save state from plugin {:?}: {}",
                            &self.id,
                            e
                        );

                        None
                    }
                };

                self.save_state.preset = preset;
            }
        }

        self.save_state.clone()
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
    ) -> Result<
        (PluginInstanceHostAudioThread, PluginHandle, FnvHashMap<ParamID, f64>),
        ActivatePluginError,
    > {
        self.can_activate()?;

        let plugin_main_thread = self.main_thread.as_mut().unwrap();

        self.save_state.activation_requested = true;

        let audio_ports = match plugin_main_thread.audio_ports_ext() {
            Ok(audio_ports) => {
                self.num_audio_in_channels = audio_ports.total_in_channels();
                self.num_audio_out_channels = audio_ports.total_out_channels();

                self.save_state.backup_audio_ports = Some(audio_ports.clone());

                audio_ports
            }
            Err(e) => {
                self.state.set(PluginState::InactiveWithError);
                self.audio_ports = None;

                return Err(ActivatePluginError::PluginFailedToGetAudioPortsExt(e));
            }
        };

        let note_ports = match plugin_main_thread.note_ports_ext() {
            Ok(note_ports) => {
                self.save_state.backup_note_ports = Some(note_ports.clone());

                note_ports
            }
            Err(e) => {
                self.state.set(PluginState::InactiveWithError);
                self.note_ports = None;

                return Err(ActivatePluginError::PluginFailedToGetNotePortsExt(e));
            }
        };

        self.audio_ports = Some(audio_ports.clone());
        self.note_ports = Some(note_ports.clone());

        let num_params = plugin_main_thread.num_params() as usize;
        let mut params: FnvHashMap<ParamID, ParamInfo> = FnvHashMap::default();
        let mut param_values: FnvHashMap<ParamID, f64> = FnvHashMap::default();

        for i in 0..num_params {
            match plugin_main_thread.param_info(i) {
                Ok(info) => match plugin_main_thread.param_value(info.stable_id) {
                    Ok(value) => {
                        let id = info.stable_id;

                        let _ = params.insert(id, info);
                        let _ = param_values.insert(id, value);
                    }
                    Err(_) => {
                        self.state.set(PluginState::InactiveWithError);

                        return Err(ActivatePluginError::PluginFailedToGetParamValue(
                            info.stable_id,
                        ));
                    }
                },
                Err(_) => {
                    self.state.set(PluginState::InactiveWithError);

                    return Err(ActivatePluginError::PluginFailedToGetParamInfo(i));
                }
            }
        }

        match plugin_main_thread.activate(sample_rate, min_frames, max_frames, coll_handle) {
            Ok(plugin_audio_thread) => {
                self.host_request.reset_deactivate();
                self.host_request.request_process();

                self.state.set(PluginState::ActiveAndSleeping);

                let mut params_ext = PluginParamsExt {
                    params,
                    ui_to_audio_param_value_tx: None,
                    ui_to_audio_param_mod_tx: None,
                };

                let (param_queues_main_thread, param_queues_audio_thread) = if num_params > 0 {
                    let (ui_to_audio_param_value_tx, ui_to_audio_param_value_rx) =
                        ReducingFnvQueue::new(num_params, coll_handle);
                    let (ui_to_audio_param_mod_tx, ui_to_audio_param_mod_rx) =
                        ReducingFnvQueue::new(num_params, coll_handle);
                    let (audio_to_main_param_value_tx, audio_to_main_param_value_rx) =
                        ReducingFnvQueue::new(num_params, coll_handle);

                    params_ext.ui_to_audio_param_value_tx = Some(ui_to_audio_param_value_tx);
                    params_ext.ui_to_audio_param_mod_tx = Some(ui_to_audio_param_mod_tx);

                    (
                        Some(ParamQueuesMainThread { audio_to_main_param_value_rx }),
                        Some(ParamQueuesAudioThread {
                            audio_to_main_param_value_tx,
                            ui_to_audio_param_value_rx,
                            ui_to_audio_param_mod_rx,
                        }),
                    )
                } else {
                    (None, None)
                };

                self.param_queues = param_queues_main_thread;

                let mut is_adjusting_parameter = FnvHashMap::default();
                is_adjusting_parameter.reserve(num_params * 2);

                let allow_outputting_transport_events =
                    plugin_audio_thread.allow_outputting_transport_events();
                let allow_outputting_automation_events =
                    plugin_audio_thread.allow_outputting_automation_events();

                Ok((
                    PluginInstanceHostAudioThread {
                        id: self.id.clone(),
                        plugin: plugin_audio_thread,
                        state: Arc::clone(&self.state),
                        param_queues: param_queues_audio_thread,
                        in_events: EventQueue::new(num_params * 3),
                        out_events: EventQueue::new(num_params * 3),
                        is_adjusting_parameter,
                        allow_outputting_transport_events,
                        allow_outputting_automation_events,
                        host_request: self.host_request.clone(),
                    },
                    PluginHandle { audio_ports, note_ports, params: params_ext },
                    param_values,
                ))
            }
            Err(e) => {
                self.state.set(PluginState::InactiveWithError);

                Err(ActivatePluginError::PluginSpecific(e))
            }
        }
    }

    pub fn schedule_deactivate(&mut self) {
        self.save_state.activation_requested = false;

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

    pub fn audio_ports_ext(&self) -> Option<&PluginAudioPortsExt> {
        if self.audio_ports.is_some() {
            self.audio_ports.as_ref()
        } else {
            self.save_state.backup_audio_ports.as_ref()
        }
    }

    pub fn note_ports_ext(&self) -> Option<&PluginNotePortsExt> {
        if self.note_ports.is_some() {
            self.note_ports.as_ref()
        } else {
            self.save_state.backup_note_ports.as_ref()
        }
    }

    pub fn on_idle(
        &mut self,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
        coll_handle: &basedrop::Handle,
    ) -> (OnIdleResult, SmallVec<[ParamModifiedInfo; 4]>) {
        let mut modified_params: SmallVec<[ParamModifiedInfo; 4]> = SmallVec::new();

        if self.main_thread.is_none() {
            if self.remove_requested {
                return (OnIdleResult::PluginReadyToRemove, modified_params);
            } else {
                return (OnIdleResult::Ok, modified_params);
            }
        }

        let plugin_main_thread = self.main_thread.as_mut().unwrap();

        let request_flags = self.host_request.load_requests_and_reset_callback();
        let state = self.state.get();

        if self.remove_requested {
            if !state.is_active() {
                return (OnIdleResult::PluginReadyToRemove, modified_params);
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

                self.param_queues = None;

                if !self.remove_requested {
                    let mut res = OnIdleResult::PluginDeactivated;

                    if self.host_request.reset_restart() {
                        match self.activate(sample_rate, min_frames, max_frames, coll_handle) {
                            Ok((audio_thread, ui_handle, param_values)) => {
                                res = OnIdleResult::PluginActivated(
                                    audio_thread,
                                    ui_handle,
                                    param_values,
                                )
                            }
                            Err(e) => res = OnIdleResult::PluginFailedToActivate(e),
                        }
                    }

                    return (res, modified_params);
                }
            }
        } else if request_flags.contains(RequestFlags::RESTART) && !self.remove_requested {
            // Wait for the audio thread part to go to sleep before
            // deactivating.
            self.host_request.request_deactivate();
        }

        if let Some(params_queue) = &mut self.param_queues {
            params_queue.audio_to_main_param_value_rx.consume(|param_id, new_value| {
                let is_gesturing = if let Some(gesture) = new_value.gesture {
                    let _ = self.gesturing_params.insert(*param_id, gesture.is_begin);
                    gesture.is_begin
                } else {
                    *self.gesturing_params.get(param_id).unwrap_or(&false)
                };

                modified_params.push(ParamModifiedInfo {
                    param_id: *param_id,
                    new_value: new_value.value,
                    is_gesturing,
                })
            });
        }

        (OnIdleResult::Ok, modified_params)
    }
}

pub(crate) enum OnIdleResult {
    Ok,
    PluginDeactivated,
    PluginActivated(PluginInstanceHostAudioThread, PluginHandle, FnvHashMap<ParamID, f64>),
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

    is_adjusting_parameter: FnvHashMap<ParamID, bool>,

    allow_outputting_transport_events: bool,
    allow_outputting_automation_events: bool,

    host_request: HostRequest,
}

impl PluginInstanceHostAudioThread {
    pub fn process<'a>(
        &mut self,
        proc_info: &ProcInfo,
        buffers: &mut ProcBuffers,
        event_in_buffers: &Option<SmallVec<[SharedBuffer<ProcEvent>; 2]>>,
        event_out_buffer: &Option<SharedBuffer<ProcEvent>>,
        note_in_buffers: &[Option<SmallVec<[SharedBuffer<ProcEvent>; 2]>>],
        note_out_buffers: &[Option<SharedBuffer<ProcEvent>>],
    ) {
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

        // Always clear event and note output buffers.
        if let Some(out_buf) = event_out_buffer {
            let mut b = unsafe { out_buf.borrow_mut() };
            b.clear();
        }
        for out_buf in note_out_buffers.iter() {
            if let Some(out_buf) = out_buf {
                let mut b = unsafe { out_buf.borrow_mut() };
                b.clear();
            }
        }

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

        let mut has_param_in_event = false;
        let mut has_note_in_event = false;

        if let Some(params_queue) = &mut self.param_queues {
            params_queue.ui_to_audio_param_value_rx.consume(|param_id, value| {
                has_param_in_event = true;

                self.in_events.push(ProcEvent::param_value(
                    EventParamValue::new(
                        // TODO: Finer values for `time` instead of just setting it to the first frame?
                        0,                   // time
                        0,                   // space_id
                        EventFlags::empty(), // event_flags
                        *param_id,           // param_id
                        // TODO: Note ID
                        -1, // note_id
                        // TODO: Port index
                        -1, // port_index
                        // TODO: Channel
                        -1, // channel
                        // TODO: Key
                        -1,          // key
                        value.value, // value
                    ),
                    None,
                ))
            });

            params_queue.ui_to_audio_param_mod_rx.consume(|param_id, value| {
                has_param_in_event = true;

                self.in_events.push(ProcEvent::param_mod(
                    EventParamMod::new(
                        // TODO: Finer values for `time` instead of just setting it to the first frame?
                        0,                   // time
                        0,                   // space_id
                        EventFlags::empty(), // event_flags
                        *param_id,           // param_id
                        // TODO: Note ID
                        -1, // note_id
                        // TODO: Port index
                        -1, // port_index
                        // TODO: Channel
                        -1, // channel
                        // TODO: Key
                        -1,          // key
                        value.value, // value
                    ),
                    None,
                ))
            });
        }

        for (port_i, in_buffers) in note_in_buffers.iter().enumerate() {
            if let Some(in_buffers) = in_buffers {
                for in_buf in in_buffers.iter() {
                    let mut b = unsafe { in_buf.borrow_mut() };

                    for mut event in b.drain(..) {
                        let mut do_add = true;
                        if let Ok(event) = ProcEvent::get_mut(&mut event) {
                            // TODO: Convert to a different note dialect if the plugin does not support
                            // the dialect in the event.
                            match event {
                                ProcEventRefMut::Note(e) => e.set_port_index(port_i as i16),
                                ProcEventRefMut::NoteExpression(e) => {
                                    e.set_port_index(port_i as i16)
                                }
                                ProcEventRefMut::Midi(e) => e.set_port_index(port_i as u16),
                                ProcEventRefMut::MidiSysex(e) => e.set_port_index(port_i as u16),
                                ProcEventRefMut::Midi2(e) => e.set_port_index(port_i as u16),
                                _ => do_add = false,
                            }
                        }

                        if do_add {
                            has_note_in_event = true;
                            self.in_events.push(event);
                        }
                    }
                }
            }
        }

        if let Some(event_in_buffers) = event_in_buffers {
            for in_buf in event_in_buffers.iter() {
                let mut b = unsafe { in_buf.borrow_mut() };

                for mut event in b.drain(..) {
                    let do_add = if let Ok(event) = ProcEvent::get_mut(&mut event) {
                        match event {
                            ProcEventRefMut::ParamValue(_, target_plugin) => {
                                if let Some(target_plugin) = target_plugin {
                                    if *target_plugin == self.id.node_ref {
                                        has_param_in_event = true;
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            }
                            ProcEventRefMut::ParamMod(_, target_plugin) => {
                                if let Some(target_plugin) = target_plugin {
                                    if *target_plugin == self.id.node_ref {
                                        has_param_in_event = true;
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            }
                            ProcEventRefMut::Transport(_) => true,
                            _ => false,
                        }
                    } else {
                        false
                    };

                    if do_add {
                        self.in_events.push(event);
                    }
                }
            }
        }

        if state == PluginState::ActiveAndWaitingForQuiet && !has_note_in_event {
            // Sync constant masks for more efficient silence checking.
            for buf in buffers.audio_in.iter_mut() {
                buf.sync_constant_mask_from_buffers();
            }

            if buffers.audio_inputs_silent(proc_info.frames) {
                self.plugin.stop_processing();

                self.state.set(PluginState::ActiveAndSleeping);
                clear_outputs(proc_info, buffers);

                if has_param_in_event {
                    self.plugin.param_flush(&self.in_events, &mut self.out_events);
                }

                self.in_events.clear();
                return;
            }
        }

        if state.is_sleeping() {
            if !request_flags.contains(RequestFlags::PROCESS) && !has_note_in_event {
                // The plugin is sleeping, there is no request to wake it up, and there
                // are no events to process.
                clear_outputs(proc_info, buffers);

                if has_param_in_event {
                    self.plugin.param_flush(&self.in_events, &mut self.out_events);
                }

                self.in_events.clear();
                return;
            }

            self.host_request.reset_process();

            if let Err(_) = self.plugin.start_processing() {
                // The plugin failed to start processing.
                self.state.set(PluginState::ActiveWithError);
                clear_outputs(proc_info, buffers);

                if has_param_in_event {
                    self.plugin.param_flush(&self.in_events, &mut self.out_events);
                }

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

        if let Some(params_queue) = &mut self.param_queues {
            // TODO: Change this to not take a closure so we don't
            // have to duplicate this code.
            params_queue.audio_to_main_param_value_tx.produce(|mut queue| {
                while let Some(mut out_event) = self.out_events.pop() {
                    let mut push_to_event_out = false;
                    let mut push_to_note_out_i = None;
                    match ProcEvent::get_mut(&mut out_event) {
                        Ok(ProcEventRefMut::ParamGesture(event)) => {
                            // TODO: Use event.time for more accurate recording of automation.

                            let is_adjusting =
                                self.is_adjusting_parameter.entry(event.param_id()).or_insert(false);

                            if event.is_begin() {
                                if *is_adjusting {
                                    log::warn!(
                                        "The plugin sent BEGIN_ADJUST twice. The event was ignored."
                                    );
                                    continue;
                                }

                                *is_adjusting = true;

                                let value = AudioToMainParamValue {
                                    value: None,
                                    gesture: Some(ParamGestureInfo { is_begin: true }),
                                };

                                queue.set_or_update(event.param_id(), value);
                            } else {
                                if !*is_adjusting {
                                    log::warn!(
                                        "The plugin sent END_ADJUST without a preceding BEGIN_ADJUST. The event was ignored."
                                    );
                                    continue;
                                }

                                *is_adjusting = false;

                                let value = AudioToMainParamValue {
                                    value: None,
                                    gesture: Some(ParamGestureInfo { is_begin: false }),
                                };

                                queue.set_or_update(event.param_id(), value);
                            }
                        }
                        Ok(ProcEventRefMut::ParamValue(event, target_plugin)) => {
                            // TODO: Use event.time for more accurate recording of automation.

                            let value = AudioToMainParamValue {
                                value: Some(event.value()),
                                gesture: None,
                            };

                            if self.allow_outputting_automation_events {
                                if target_plugin.is_some() {
                                    if event_out_buffer.is_some() {
                                        push_to_event_out = true;
                                    }
                                } else {
                                    // If `target_plugin` is `None`, then it means that this is a parameter
                                    // event for one of this plugin's internal parameters, and is meant to
                                    // be sent to the UI, not another plugin.
                                    queue.set_or_update(event.param_id(), value);
                                }
                            } else {
                                queue.set_or_update(event.param_id(), value);
                            }
                        }
                        Ok(ProcEventRefMut::ParamMod(_, _)) => {
                            if event_out_buffer.is_some() && self.allow_outputting_automation_events {
                                push_to_event_out = true;
                            }
                        }
                        Ok(ProcEventRefMut::Transport(_)) => {
                            if event_out_buffer.is_some() && self.allow_outputting_transport_events {
                                push_to_event_out = true;
                            }
                        }
                        Ok(ProcEventRefMut::Note(event)) => {
                            if let Some(Some(_)) = note_out_buffers.get(event.port_index() as usize) {
                                push_to_note_out_i = Some(event.port_index() as usize);
                            }
                        }
                        Ok(ProcEventRefMut::NoteExpression(event)) => {
                            if let Some(Some(_)) = note_out_buffers.get(event.port_index() as usize) {
                                push_to_note_out_i = Some(event.port_index() as usize);
                            }
                        }
                        Ok(ProcEventRefMut::Midi(event)) => {
                            if let Some(Some(_)) = note_out_buffers.get(event.port_index() as usize) {
                                push_to_note_out_i = Some(event.port_index() as usize);
                            }
                        }
                        Ok(ProcEventRefMut::MidiSysex(event)) => {
                            if let Some(Some(_)) = note_out_buffers.get(event.port_index() as usize) {
                                push_to_note_out_i = Some(event.port_index() as usize);
                            }
                        }
                        Ok(ProcEventRefMut::Midi2(event)) => {
                            if let Some(Some(_)) = note_out_buffers.get(event.port_index() as usize) {
                                push_to_note_out_i = Some(event.port_index() as usize);
                            }
                        }
                        _ => {}
                    }

                    if push_to_event_out {
                        let out_buf = event_out_buffer.as_ref().unwrap();

                        let mut b = unsafe { out_buf.borrow_mut() };

                        b.push(out_event);
                    } else if let Some(buf_i) = push_to_note_out_i {
                        let out_buf = note_out_buffers[buf_i].as_ref().unwrap();

                        let mut b = unsafe { out_buf.borrow_mut() };

                        b.push(out_event);
                    }
                }
            });
        } else {
            while let Some(mut out_event) = self.out_events.pop() {
                let mut push_to_event_out = false;
                let mut push_to_note_out_i = None;
                match ProcEvent::get_mut(&mut out_event) {
                    Ok(ProcEventRefMut::ParamValue(_, target_plugin)) => {
                        if self.allow_outputting_automation_events {
                            // If `target_plugin` is `None`, then it means that this is a parameter
                            // event for one of this plugin's internal parameters, and is meant to
                            // be sent to the UI, not another plugin.
                            if target_plugin.is_some() {
                                if event_out_buffer.is_some() {
                                    push_to_event_out = true;
                                }
                            }
                        }
                    }
                    Ok(ProcEventRefMut::ParamMod(_, _)) => {
                        if event_out_buffer.is_some() && self.allow_outputting_automation_events {
                            push_to_event_out = true;
                        }
                    }
                    Ok(ProcEventRefMut::Transport(_)) => {
                        if event_out_buffer.is_some() && self.allow_outputting_transport_events {
                            push_to_event_out = true;
                        }
                    }
                    Ok(ProcEventRefMut::Note(event)) => {
                        if let Some(Some(_)) = note_out_buffers.get(event.port_index() as usize) {
                            push_to_note_out_i = Some(event.port_index() as usize);
                        }
                    }
                    Ok(ProcEventRefMut::NoteExpression(event)) => {
                        if let Some(Some(_)) = note_out_buffers.get(event.port_index() as usize) {
                            push_to_note_out_i = Some(event.port_index() as usize);
                        }
                    }
                    Ok(ProcEventRefMut::Midi(event)) => {
                        if let Some(Some(_)) = note_out_buffers.get(event.port_index() as usize) {
                            push_to_note_out_i = Some(event.port_index() as usize);
                        }
                    }
                    Ok(ProcEventRefMut::MidiSysex(event)) => {
                        if let Some(Some(_)) = note_out_buffers.get(event.port_index() as usize) {
                            push_to_note_out_i = Some(event.port_index() as usize);
                        }
                    }
                    Ok(ProcEventRefMut::Midi2(event)) => {
                        if let Some(Some(_)) = note_out_buffers.get(event.port_index() as usize) {
                            push_to_note_out_i = Some(event.port_index() as usize);
                        }
                    }
                    _ => {}
                }

                if push_to_event_out {
                    let out_buf = event_out_buffer.as_ref().unwrap();

                    let mut b = unsafe { out_buf.borrow_mut() };

                    b.push(out_event);
                } else if let Some(buf_i) = push_to_note_out_i {
                    let out_buf = note_out_buffers[buf_i].as_ref().unwrap();

                    let mut b = unsafe { out_buf.borrow_mut() };

                    b.push(out_event);
                }
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
    PluginFailedToGetAudioPortsExt(Box<dyn Error + Send>),
    PluginFailedToGetNotePortsExt(Box<dyn Error + Send>),
    PluginFailedToGetParamInfo(usize),
    PluginFailedToGetParamValue(ParamID),
    PluginSpecific(Box<dyn Error + Send>),
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
            ActivatePluginError::PluginFailedToGetNotePortsExt(e) => {
                write!(f, "plugin returned error while getting note ports extension: {:?}", e)
            }
            ActivatePluginError::PluginFailedToGetParamInfo(index) => {
                write!(f, "plugin returned error while getting parameter info at index: {}", index)
            }
            ActivatePluginError::PluginFailedToGetParamValue(param_id) => {
                write!(
                    f,
                    "plugin returned error while getting parameter value with ID: {:?}",
                    param_id
                )
            }
            ActivatePluginError::PluginSpecific(e) => {
                write!(f, "plugin returned error while activating: {:?}", e)
            }
        }
    }
}

impl From<Box<dyn Error + Send>> for ActivatePluginError {
    fn from(e: Box<dyn Error + Send>) -> Self {
        ActivatePluginError::PluginSpecific(e)
    }
}
