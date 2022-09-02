use audio_graph::{AudioGraphHelper, EdgeID, PortID};
use basedrop::Shared;
use dropseed_plugin_api::ext::audio_ports::{MainPortsLayout, PluginAudioPortsExt};
use dropseed_plugin_api::ext::note_ports::PluginNotePortsExt;
use dropseed_plugin_api::ext::params::{ParamID, ParamInfo, ParamInfoFlags};
use dropseed_plugin_api::transport::TempoMap;
use dropseed_plugin_api::{
    DSPluginSaveState, HostRequestChannelReceiver, HostRequestFlags, PluginInstanceID,
    PluginMainThread,
};
use fnv::{FnvHashMap, FnvHashSet};
use meadowlark_core_types::time::SampleRate;
use smallvec::SmallVec;

use crate::engine::{OnIdleEvent, PluginActivatedStatus};
use crate::graph::{ChannelID, DSEdgeID, PortType};
use crate::utils::thread_id::SharedThreadIDs;

use super::channel::{
    MainToProcParamValue, PlugHostChannelMainThread, PluginActiveState, SharedPluginHostProcThread,
};
use super::error::{ActivatePluginError, SetParamValueError, ShowGuiError};

/// The references to this plugin's ports in the audio graph.
pub(crate) struct PluginHostPortIDs {
    pub channel_id_to_port_id: FnvHashMap<ChannelID, PortID>,
    pub port_id_to_channel_id: FnvHashMap<PortID, ChannelID>,
    pub main_audio_in_port_ids: Vec<PortID>,
    pub main_audio_out_port_ids: Vec<PortID>,
    pub main_note_in_port_id: Option<PortID>,
    pub main_note_out_port_id: Option<PortID>,
    pub automation_in_port_id: Option<PortID>,
    pub automation_out_port_id: Option<PortID>,
}

impl PluginHostPortIDs {
    pub fn new() -> Self {
        Self {
            channel_id_to_port_id: FnvHashMap::default(),
            port_id_to_channel_id: FnvHashMap::default(),
            main_audio_in_port_ids: Vec::new(),
            main_audio_out_port_ids: Vec::new(),
            main_note_in_port_id: None,
            main_note_out_port_id: None,
            automation_in_port_id: None,
            automation_out_port_id: None,
        }
    }
}

pub enum LoadedState {
    Loaded { params: FnvHashMap<ParamID, ParamInfo>, latency: i64 },
    Unloaded,
}

pub struct PluginHostMainThread {
    id: PluginInstanceID,

    loaded_state: LoadedState,

    plug_main_thread: Box<dyn PluginMainThread>,

    port_ids: PluginHostPortIDs,
    next_port_id: u32,
    free_port_ids: Vec<PortID>,

    channel: PlugHostChannelMainThread,

    save_state: DSPluginSaveState,

    gesturing_params: FnvHashMap<ParamID, bool>,

    num_audio_in_channels: usize,
    num_audio_out_channels: usize,

    host_request_rx: HostRequestChannelReceiver,
    remove_requested: bool,
    save_state_dirty: bool,
    restarting: bool,
}

impl PluginHostMainThread {
    pub(crate) fn new(
        id: PluginInstanceID,
        save_state: DSPluginSaveState,
        mut plug_main_thread: Box<dyn PluginMainThread>,
        host_request_rx: HostRequestChannelReceiver,
        plugin_loaded: bool,
        coll_handle: &basedrop::Handle,
    ) -> Self {
        if let Some(save_state) = save_state.raw_state.clone() {
            match plug_main_thread.load_save_state(save_state) {
                Ok(()) => {
                    log::trace!("Plugin {:?} successfully loaded save state", &id);
                }
                Err(e) => {
                    log::error!("Plugin {:?} failed to load save state: {}", &id, e);
                }
            }
        }

        let (num_audio_in_channels, num_audio_out_channels) =
            if let Some(audio_ports_ext) = &save_state.backup_audio_ports {
                (audio_ports_ext.total_in_channels(), audio_ports_ext.total_out_channels())
            } else {
                (0, 0)
            };

        let loaded_state = if plugin_loaded {
            LoadedState::Loaded { params: FnvHashMap::default(), latency: 0 }
        } else {
            LoadedState::Unloaded
        };

        Self {
            id,
            plug_main_thread,
            port_ids: PluginHostPortIDs::new(),
            next_port_id: 0,
            free_port_ids: Vec::new(),
            loaded_state,
            channel: PlugHostChannelMainThread::new(coll_handle),
            save_state,
            gesturing_params: FnvHashMap::default(),
            num_audio_in_channels,
            num_audio_out_channels,
            host_request_rx,
            remove_requested: false,
            save_state_dirty: false,
            restarting: false,
        }
    }

    /// Tell the plugin to load the given save state.
    ///
    /// This will return `Err(e)` if the plugin failed to load the given
    /// save state.
    pub fn load_save_state(&mut self, state: Vec<u8>) -> Result<(), String> {
        self.save_state_dirty = true;
        self.plug_main_thread.load_save_state(state)
    }

    /// This will return `true` if the plugin's save state has changed
    /// since the last time its save state was collected.
    pub fn is_save_state_dirty(&self) -> bool {
        self.save_state_dirty
    }

    /// Collect the save state of this plugin.
    pub fn collect_save_state(&mut self) -> DSPluginSaveState {
        if self.save_state_dirty {
            self.save_state_dirty = false;

            let raw_state = match self.plug_main_thread.collect_save_state() {
                Ok(raw_state) => raw_state,
                Err(e) => {
                    log::error!("Failed to collect save state from plugin {:?}: {}", &self.id, e);

                    None
                }
            };

            self.save_state.raw_state = raw_state;
        }

        self.save_state.clone()
    }

    /// Set the value of the given parameter.
    ///
    /// If successful, this returns the actual (clamped) value that the
    /// plugin accepted.
    pub fn set_param_value(
        &mut self,
        param_id: ParamID,
        value: f64,
    ) -> Result<f64, SetParamValueError> {
        if let LoadedState::Loaded { params, .. } = &self.loaded_state {
            if let Some(param_info) = params.get(&param_id) {
                if param_info.flags.contains(ParamInfoFlags::IS_READONLY) {
                    Err(SetParamValueError::ParamIsReadOnly(param_id))
                } else {
                    let value = value.clamp(param_info.min_value, param_info.max_value);

                    if let Some(param_queues) = &mut self.channel.param_queues {
                        param_queues
                            .to_proc_param_value_tx
                            .set(param_id, MainToProcParamValue { value });
                        param_queues.to_proc_param_value_tx.producer_done();
                    } else {
                        // TODO: Flush parameters on main thread.
                    }

                    self.save_state_dirty = true;

                    Ok(value)
                }
            } else {
                Err(SetParamValueError::ParamDoesNotExist(param_id))
            }
        } else {
            Err(SetParamValueError::PluginNotLoaded)
        }
    }

    /// Set the modulation amount on the given parameter.
    ///
    /// If successful, this returns the actual (clamped) modulation
    /// amount that the plugin accepted.
    pub fn set_param_mod_amount(
        &mut self,
        param_id: ParamID,
        mod_amount: f64,
    ) -> Result<f64, SetParamValueError> {
        if let LoadedState::Loaded { params, .. } = &self.loaded_state {
            if let Some(param_info) = params.get(&param_id) {
                if param_info.flags.contains(ParamInfoFlags::IS_MODULATABLE) {
                    Err(SetParamValueError::ParamIsNotModulatable(param_id))
                } else {
                    // TODO: Clamp value?

                    if let Some(param_queues) = &mut self.channel.param_queues {
                        param_queues
                            .to_proc_param_mod_tx
                            .set(param_id, MainToProcParamValue { value: mod_amount });
                        param_queues.to_proc_param_mod_tx.producer_done();
                    } else {
                        // TODO: Flush parameters on main thread.
                    }

                    Ok(mod_amount)
                }
            } else {
                Err(SetParamValueError::ParamDoesNotExist(param_id))
            }
        } else {
            Err(SetParamValueError::PluginNotLoaded)
        }
    }

    /// Get the display text for the given parameter with the given
    /// value.
    pub fn param_value_to_text(
        &self,
        param_id: ParamID,
        value: f64,
        text_buffer: &mut String,
    ) -> Result<(), String> {
        self.plug_main_thread.param_value_to_text(param_id, value, text_buffer)
    }

    /// Conver the given text input to a value for this parameter.
    pub fn param_text_to_value(&self, param_id: ParamID, text_input: &str) -> Option<f64> {
        self.plug_main_thread.param_text_to_value(param_id, text_input)
    }

    /// Tell the plugin to open its custom GUI.
    pub fn show_gui(&mut self) -> Result<(), ShowGuiError> {
        if !self.plug_main_thread.is_gui_open() {
            if let Err(e) = self.plug_main_thread.open_gui(None) {
                return Err(ShowGuiError::HostError(e));
            }
            Ok(())
        } else {
            Err(ShowGuiError::AlreadyOpen)
        }
    }

    /// Tell the plugin to close its custom GUI.
    pub fn close_gui(&mut self) {
        if self.plug_main_thread.is_gui_open() {
            self.plug_main_thread.close_gui();
        }
    }

    /// Returns `true` if this plugin has a custom GUI that can be
    /// opened in a floating window.
    pub fn has_gui(&self) -> bool {
        self.plug_main_thread.has_gui()
    }

    /// Returns `Ok(())` if the plugin can be activated right now.
    pub fn can_activate(&self) -> Result<(), ActivatePluginError> {
        // TODO: without this check it seems something is attempting to activate the plugin twice
        if self.channel.shared_state.get_active_state() == PluginActiveState::Active {
            return Err(ActivatePluginError::AlreadyActive);
        }
        Ok(())
    }

    // TODO: let the user manually activate an inactive plugin
    pub(crate) fn activate(
        &mut self,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
        graph_helper: &mut AudioGraphHelper,
        edge_id_to_ds_edge_id: &mut FnvHashMap<EdgeID, DSEdgeID>,
        thread_ids: SharedThreadIDs,
        coll_handle: &basedrop::Handle,
    ) -> Result<PluginActivatedStatus, ActivatePluginError> {
        self.can_activate()?;

        let audio_ports = match self.plug_main_thread.audio_ports_ext() {
            Ok(audio_ports) => audio_ports,
            Err(e) => {
                self.channel.shared_state.set_active_state(PluginActiveState::InactiveWithError);
                self.channel.param_queues = None;

                return Err(ActivatePluginError::PluginFailedToGetAudioPortsExt(e));
            }
        };

        let note_ports = match self.plug_main_thread.note_ports_ext() {
            Ok(note_ports) => note_ports,
            Err(e) => {
                self.channel.shared_state.set_active_state(PluginActiveState::InactiveWithError);
                self.channel.param_queues = None;

                return Err(ActivatePluginError::PluginFailedToGetNotePortsExt(e));
            }
        };

        let num_params = self.plug_main_thread.num_params() as usize;
        let mut new_params: FnvHashMap<ParamID, ParamInfo> = FnvHashMap::default();
        let mut param_values: Vec<(ParamInfo, f64)> = Vec::with_capacity(num_params);

        for i in 0..num_params {
            match self.plug_main_thread.param_info(i) {
                Ok(info) => match self.plug_main_thread.param_value(info.stable_id) {
                    Ok(value) => {
                        let id = info.stable_id;

                        new_params.insert(id, info.clone());
                        param_values.push((info, value));
                    }
                    Err(_) => {
                        self.channel
                            .shared_state
                            .set_active_state(PluginActiveState::InactiveWithError);
                        self.channel.param_queues = None;

                        return Err(ActivatePluginError::PluginFailedToGetParamValue(
                            info.stable_id,
                        ));
                    }
                },
                Err(_) => {
                    self.channel
                        .shared_state
                        .set_active_state(PluginActiveState::InactiveWithError);
                    self.channel.param_queues = None;

                    return Err(ActivatePluginError::PluginFailedToGetParamInfo(i));
                }
            }
        }

        let latency = self.plug_main_thread.latency();

        let (removed_edges, mut needs_recompile) = match self.sync_ports_in_graph(
            graph_helper,
            edge_id_to_ds_edge_id,
            &audio_ports,
            &note_ports,
            latency,
            coll_handle,
        ) {
            Ok((removed_edges, needs_recompile)) => (removed_edges, needs_recompile),
            Err(e) => {
                self.channel.shared_state.set_active_state(PluginActiveState::InactiveWithError);
                self.channel.param_queues = None;

                return Err(e);
            }
        };

        match self.plug_main_thread.activate(sample_rate, min_frames, max_frames, coll_handle) {
            Ok(info) => {
                self.channel.shared_state.set_active_state(PluginActiveState::Active);

                self.channel.create_process_thread(
                    info.processor,
                    self.id.unique_id(),
                    num_params,
                    thread_ids,
                    coll_handle,
                );

                let new_latency = if let LoadedState::Loaded { params, latency: old_latency } =
                    &mut self.loaded_state
                {
                    let new_latency = if *old_latency != latency {
                        *old_latency = latency;
                        needs_recompile = true;
                        Some(latency)
                    } else {
                        None
                    };

                    *params = new_params;

                    new_latency
                } else {
                    None
                };

                let audio_ports_changed =
                    if let Some(old_audio_ports) = &self.save_state.backup_audio_ports {
                        &audio_ports != old_audio_ports
                    } else {
                        true
                    };
                let note_ports_changed =
                    if let Some(old_note_ports) = &self.save_state.backup_note_ports {
                        &note_ports != old_note_ports
                    } else {
                        true
                    };

                let new_audio_ports_ext = if audio_ports_changed {
                    self.save_state_dirty = true;

                    self.num_audio_in_channels = audio_ports.total_in_channels();
                    self.num_audio_out_channels = audio_ports.total_out_channels();

                    self.save_state.backup_audio_ports = Some(audio_ports.clone());
                    Some(audio_ports)
                } else {
                    None
                };
                let new_note_ports_ext = if note_ports_changed {
                    self.save_state_dirty = true;

                    self.save_state.backup_note_ports = Some(note_ports.clone());
                    Some(note_ports)
                } else {
                    None
                };

                Ok(PluginActivatedStatus {
                    new_parameters: param_values,
                    new_audio_ports_ext,
                    new_note_ports_ext,
                    internal_handle: info.internal_handle,
                    has_gui: self.plug_main_thread.has_gui(),
                    new_latency,
                    removed_edges,
                    caused_recompile: needs_recompile,
                })
            }
            Err(e) => {
                self.channel.shared_state.set_active_state(PluginActiveState::InactiveWithError);
                self.channel.param_queues = None;

                Err(ActivatePluginError::PluginSpecific(e))
            }
        }
    }

    /// Get the audio port configuration on this plugin.
    ///
    /// This will return `None` if this plugin is unloaded and there
    /// exists no backup of the audio ports extension.
    pub fn audio_ports_ext(&self) -> Option<&PluginAudioPortsExt> {
        self.save_state.backup_audio_ports.as_ref()
    }

    /// Get the note port configuration on this plugin.
    ///
    /// This will return `None` if this plugin is unloaded and there
    /// exists no backup of the note ports extension.
    pub fn note_ports_ext(&self) -> Option<&PluginNotePortsExt> {
        self.save_state.backup_note_ports.as_ref()
    }

    /// The total number of audio input channels on this plugin.
    pub fn num_audio_in_channels(&self) -> usize {
        self.num_audio_in_channels
    }

    /// The total number of audio output channels on this plugin.
    pub fn num_audio_out_channels(&self) -> usize {
        self.num_audio_out_channels
    }

    /// The unique ID for this plugin instance.
    pub fn id(&self) -> &PluginInstanceID {
        &self.id
    }

    pub(crate) fn on_idle(
        &mut self,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
        coll_handle: &basedrop::Handle,
        graph_helper: &mut AudioGraphHelper,
        events_out: &mut SmallVec<[OnIdleEvent; 32]>,
        edge_id_to_ds_edge_id: &mut FnvHashMap<EdgeID, DSEdgeID>,
        thread_ids: &SharedThreadIDs,
    ) -> (OnIdleResult, SmallVec<[ParamModifiedInfo; 4]>) {
        let mut modified_params: SmallVec<[ParamModifiedInfo; 4]> = SmallVec::new();

        let request_flags = self.host_request_rx.fetch_requests();
        let mut active_state = self.channel.shared_state.get_active_state();

        if request_flags.contains(HostRequestFlags::MARK_DIRTY) {
            self.save_state_dirty = true;
        }

        if request_flags.contains(HostRequestFlags::CALLBACK) {
            self.plug_main_thread.on_main_thread();
        }

        if request_flags.contains(HostRequestFlags::RESCAN_PARAMS) {
            // TODO
        }

        // We just do a full restart and rescan for all "rescan port" requests for
        // simplicity. I don't expect plugins to change the state of their ports
        // often anyway.
        if request_flags.intersects(HostRequestFlags::RESTART | HostRequestFlags::RESCAN_PORTS) {
            self.restarting = true;
            self.schedule_deactivate(coll_handle);
        }

        if request_flags.intersects(HostRequestFlags::GUI_CLOSED | HostRequestFlags::GUI_DESTROYED)
        {
            self.plug_main_thread
                .on_gui_closed(request_flags.contains(HostRequestFlags::GUI_DESTROYED));

            events_out.push(OnIdleEvent::PluginGuiClosed { plugin_id: self.id.clone() });
        }

        if let Some(params_queue) = &mut self.channel.param_queues {
            params_queue.from_proc_param_value_rx.consume(|param_id, new_value| {
                let is_gesturing = if let Some(gesture) = new_value.gesture {
                    let _ = self.gesturing_params.insert(*param_id, gesture.is_begin);

                    if !gesture.is_begin {
                        // Only mark the state dirty once the user has finished adjusting
                        // the parameter.
                        self.save_state_dirty = true;
                    }

                    gesture.is_begin
                } else {
                    self.save_state_dirty = true;

                    *self.gesturing_params.get(param_id).unwrap_or(&false)
                };

                modified_params.push(ParamModifiedInfo {
                    param_id: *param_id,
                    new_value: new_value.value,
                    is_gesturing,
                })
            });
        }

        if active_state == PluginActiveState::DroppedAndReadyToDeactivate {
            // Safe to deactivate now.
            self.plug_main_thread.deactivate();

            self.channel.shared_state.set_active_state(PluginActiveState::Inactive);

            self.channel.drop_process_thread_pointer(coll_handle);
            self.save_state_dirty = true;

            if !self.remove_requested {
                let mut res = OnIdleResult::PluginDeactivated;

                if self.restarting || request_flags.contains(HostRequestFlags::PROCESS) {
                    match self.activate(
                        sample_rate,
                        min_frames,
                        max_frames,
                        graph_helper,
                        edge_id_to_ds_edge_id,
                        thread_ids.clone(),
                        coll_handle,
                    ) {
                        Ok(r) => {
                            res = OnIdleResult::PluginActivated(r);
                        }
                        Err(e) => res = OnIdleResult::PluginFailedToActivate(e),
                    }
                }

                return (res, modified_params);
            } else {
                return (OnIdleResult::PluginReadyToRemove, modified_params);
            }
        } else if request_flags.contains(HostRequestFlags::PROCESS)
            && !self.remove_requested
            && !self.restarting
        {
            if active_state == PluginActiveState::Active {
                self.channel.shared_state.start_processing();
            } else if active_state == PluginActiveState::Inactive
                || active_state == PluginActiveState::InactiveWithError
            {
                let res = match self.activate(
                    sample_rate,
                    min_frames,
                    max_frames,
                    graph_helper,
                    edge_id_to_ds_edge_id,
                    thread_ids.clone(),
                    coll_handle,
                ) {
                    Ok(r) => {
                        self.save_state_dirty = true;

                        OnIdleResult::PluginActivated(r)
                    }
                    Err(e) => OnIdleResult::PluginFailedToActivate(e),
                };

                return (res, modified_params);
            }
        }

        (OnIdleResult::Ok, modified_params)
    }

    pub(crate) fn schedule_deactivate(&mut self, coll_handle: &basedrop::Handle) {
        if self.channel.shared_state.get_active_state() != PluginActiveState::Active {
            return;
        }

        // Allow the plugin's audio thread to be dropped when the new schedule is
        // sent.
        //
        // Note this doesn't actually drop the process thread. It only drops this
        // struct's pointer to the process thread so that when the process thread
        // drops its shared pointer, it will be collected by the garbage
        // collector.
        self.channel.drop_process_thread_pointer(coll_handle);

        // Wait for the audio thread part to go to sleep before deactivating.
        self.channel.shared_state.set_active_state(PluginActiveState::WaitingToDrop);
    }

    pub(crate) fn schedule_remove(&mut self, coll_handle: &basedrop::Handle) {
        self.remove_requested = true;

        self.schedule_deactivate(coll_handle);
    }

    pub(crate) fn update_tempo_map(&mut self, new_tempo_map: &Shared<TempoMap>) {
        self.plug_main_thread.update_tempo_map(new_tempo_map);
    }

    pub(crate) fn shared_processor(&self) -> &SharedPluginHostProcThread {
        self.channel.shared_processor()
    }

    pub(crate) fn port_ids(&self) -> &PluginHostPortIDs {
        &self.port_ids
    }

    pub(crate) fn sync_ports_in_graph(
        &mut self,
        graph_helper: &mut AudioGraphHelper,
        edge_id_to_ds_edge_id: &mut FnvHashMap<EdgeID, DSEdgeID>,
        new_audio_ports: &PluginAudioPortsExt,
        new_note_ports: &PluginNotePortsExt,
        new_latency: i64,
        coll_handle: &basedrop::Handle,
    ) -> Result<(Vec<DSEdgeID>, bool), ActivatePluginError> {
        let mut needs_recompile = false;

        let mut id_alias_check: FnvHashSet<u32> = FnvHashSet::default();
        if let Some(audio_ports) = &self.save_state.backup_audio_ports {
            for audio_in_port in audio_ports.inputs.iter() {
                if !id_alias_check.insert(audio_in_port.stable_id) {
                    self.schedule_deactivate(coll_handle);
                    return Err(ActivatePluginError::AudioPortsExtDuplicateID {
                        is_input: true,
                        id: audio_in_port.stable_id,
                    });
                }
            }
            id_alias_check.clear();
            for audio_out_port in audio_ports.outputs.iter() {
                if !id_alias_check.insert(audio_out_port.stable_id) {
                    self.schedule_deactivate(coll_handle);
                    return Err(ActivatePluginError::AudioPortsExtDuplicateID {
                        is_input: false,
                        id: audio_out_port.stable_id,
                    });
                }
            }
        }
        id_alias_check.clear();
        if let Some(note_ports) = &self.save_state.backup_note_ports {
            for note_in_port in note_ports.inputs.iter() {
                if !id_alias_check.insert(note_in_port.stable_id) {
                    self.schedule_deactivate(coll_handle);
                    return Err(ActivatePluginError::NotePortsExtDuplicateID {
                        is_input: true,
                        id: note_in_port.stable_id,
                    });
                }
            }
            id_alias_check.clear();
            for note_out_port in note_ports.outputs.iter() {
                if !id_alias_check.insert(note_out_port.stable_id) {
                    self.schedule_deactivate(coll_handle);
                    return Err(ActivatePluginError::NotePortsExtDuplicateID {
                        is_input: false,
                        id: note_out_port.stable_id,
                    });
                }
            }
        }

        graph_helper.set_node_latency(self.id._node_id().into(), new_latency as f64).unwrap();

        let mut prev_channel_ids = self.port_ids.channel_id_to_port_id.clone();

        self.port_ids.channel_id_to_port_id.clear();
        self.port_ids.port_id_to_channel_id.clear();
        self.port_ids.automation_in_port_id = None;
        self.port_ids.automation_out_port_id = None;
        self.port_ids.main_audio_in_port_ids.clear();
        self.port_ids.main_audio_out_port_ids.clear();
        self.port_ids.main_note_in_port_id = None;
        self.port_ids.main_note_out_port_id = None;

        for (audio_port_i, audio_in_port) in new_audio_ports.inputs.iter().enumerate() {
            for channel_i in 0..audio_in_port.channels {
                let channel_id = ChannelID {
                    stable_id: audio_in_port.stable_id,
                    port_type: PortType::Audio,
                    is_input: true,
                    channel: channel_i,
                };

                let port_id = if let Some(port_id) = prev_channel_ids.remove(&channel_id) {
                    port_id
                } else {
                    needs_recompile = true;

                    let new_port_id = self.free_port_ids.pop().unwrap_or_else(|| {
                        self.next_port_id += 1;
                        PortID(self.next_port_id - 1)
                    });

                    graph_helper
                        .add_port(
                            self.id._node_id().into(),
                            new_port_id,
                            PortType::Audio.as_type_idx(),
                            true,
                        )
                        .unwrap();

                    new_port_id
                };

                self.port_ids.channel_id_to_port_id.insert(channel_id, port_id);
                self.port_ids.port_id_to_channel_id.insert(port_id, channel_id);

                if audio_port_i == 0 {
                    match new_audio_ports.main_ports_layout {
                        MainPortsLayout::InOut | MainPortsLayout::InOnly => {
                            self.port_ids.main_audio_in_port_ids.push(port_id);
                        }
                        _ => {}
                    }
                }
            }
        }

        for (audio_port_i, audio_out_port) in new_audio_ports.outputs.iter().enumerate() {
            for channel_i in 0..audio_out_port.channels {
                let channel_id = ChannelID {
                    stable_id: audio_out_port.stable_id,
                    port_type: PortType::Audio,
                    is_input: false,
                    channel: channel_i,
                };

                let port_id = if let Some(port_id) = prev_channel_ids.remove(&channel_id) {
                    port_id
                } else {
                    needs_recompile = true;

                    let new_port_id = self.free_port_ids.pop().unwrap_or_else(|| {
                        self.next_port_id += 1;
                        PortID(self.next_port_id - 1)
                    });

                    graph_helper
                        .add_port(
                            self.id._node_id().into(),
                            new_port_id,
                            PortType::Audio.as_type_idx(),
                            false,
                        )
                        .unwrap();

                    new_port_id
                };

                self.port_ids.channel_id_to_port_id.insert(channel_id, port_id);
                self.port_ids.port_id_to_channel_id.insert(port_id, channel_id);

                if audio_port_i == 0 {
                    match new_audio_ports.main_ports_layout {
                        MainPortsLayout::InOut | MainPortsLayout::OutOnly => {
                            self.port_ids.main_audio_out_port_ids.push(port_id);
                        }
                        _ => {}
                    }
                }
            }
        }

        const IN_AUTOMATION_CHANNEL_ID: ChannelID =
            ChannelID { port_type: PortType::Automation, stable_id: 0, is_input: true, channel: 0 };
        const OUT_AUTOMATION_CHANNEL_ID: ChannelID = ChannelID {
            port_type: PortType::Automation,
            stable_id: 0,
            is_input: false,
            channel: 0,
        };

        // Plugins always have one automation in port.
        let automation_in_port_id =
            if let Some(port_id) = prev_channel_ids.remove(&IN_AUTOMATION_CHANNEL_ID) {
                port_id
            } else {
                needs_recompile = true;

                let new_port_id = self.free_port_ids.pop().unwrap_or_else(|| {
                    self.next_port_id += 1;
                    PortID(self.next_port_id - 1)
                });

                graph_helper
                    .add_port(
                        self.id._node_id().into(),
                        new_port_id,
                        PortType::Automation.as_type_idx(),
                        true,
                    )
                    .unwrap();

                new_port_id
            };
        self.port_ids.channel_id_to_port_id.insert(IN_AUTOMATION_CHANNEL_ID, automation_in_port_id);
        self.port_ids.port_id_to_channel_id.insert(automation_in_port_id, IN_AUTOMATION_CHANNEL_ID);
        self.port_ids.automation_in_port_id = Some(automation_in_port_id);

        if self.plug_main_thread.has_automation_out_port() {
            let automation_out_port_id =
                if let Some(port_id) = prev_channel_ids.remove(&OUT_AUTOMATION_CHANNEL_ID) {
                    port_id
                } else {
                    needs_recompile = true;

                    let new_port_id = self.free_port_ids.pop().unwrap_or_else(|| {
                        self.next_port_id += 1;
                        PortID(self.next_port_id - 1)
                    });

                    graph_helper
                        .add_port(
                            self.id._node_id().into(),
                            new_port_id,
                            PortType::Automation.as_type_idx(),
                            false,
                        )
                        .unwrap();

                    new_port_id
                };
            self.port_ids
                .channel_id_to_port_id
                .insert(OUT_AUTOMATION_CHANNEL_ID, automation_out_port_id);
            self.port_ids
                .port_id_to_channel_id
                .insert(automation_out_port_id, OUT_AUTOMATION_CHANNEL_ID);
            self.port_ids.automation_out_port_id = Some(automation_out_port_id);
        }

        for (i, note_in_port) in new_note_ports.inputs.iter().enumerate() {
            let channel_id = ChannelID {
                port_type: PortType::Note,
                stable_id: note_in_port.stable_id,
                is_input: true,
                channel: 0,
            };

            let port_id = if let Some(port_id) = prev_channel_ids.remove(&channel_id) {
                port_id
            } else {
                needs_recompile = true;

                let new_port_id = self.free_port_ids.pop().unwrap_or_else(|| {
                    self.next_port_id += 1;
                    PortID(self.next_port_id - 1)
                });

                graph_helper
                    .add_port(
                        self.id._node_id().into(),
                        new_port_id,
                        PortType::Note.as_type_idx(),
                        true,
                    )
                    .unwrap();

                new_port_id
            };

            self.port_ids.channel_id_to_port_id.insert(channel_id, port_id);
            self.port_ids.port_id_to_channel_id.insert(port_id, channel_id);

            if i == 0 {
                self.port_ids.main_note_in_port_id = Some(port_id);
            }
        }

        for (i, note_out_port) in new_note_ports.outputs.iter().enumerate() {
            let channel_id = ChannelID {
                port_type: PortType::Note,
                stable_id: note_out_port.stable_id,
                is_input: false,
                channel: 0,
            };

            let port_id = if let Some(port_id) = prev_channel_ids.remove(&channel_id) {
                port_id
            } else {
                needs_recompile = true;

                let new_port_id = self.free_port_ids.pop().unwrap_or_else(|| {
                    self.next_port_id += 1;
                    PortID(self.next_port_id - 1)
                });

                graph_helper
                    .add_port(
                        self.id._node_id().into(),
                        new_port_id,
                        PortType::Note.as_type_idx(),
                        false,
                    )
                    .unwrap();

                new_port_id
            };

            self.port_ids.channel_id_to_port_id.insert(channel_id, port_id);
            self.port_ids.port_id_to_channel_id.insert(port_id, channel_id);

            if i == 0 {
                self.port_ids.main_note_out_port_id = Some(port_id);
            }
        }

        if !prev_channel_ids.is_empty() {
            needs_recompile = true;
        }

        let mut removed_edges: Vec<DSEdgeID> = Vec::new();
        for (_, port_to_remove_id) in prev_channel_ids.drain() {
            let removed_edges_res =
                graph_helper.remove_port(self.id._node_id().into(), port_to_remove_id).unwrap();

            for edge_id in removed_edges_res.iter() {
                if let Some(ds_edge_id) = edge_id_to_ds_edge_id.remove(edge_id) {
                    removed_edges.push(ds_edge_id);
                } else {
                    panic!(
                        "Helper disconnected an edge that doesn't exist in graph: {:?}",
                        edge_id
                    );
                }
            }

            self.free_port_ids.push(port_to_remove_id);
        }

        Ok((removed_edges, needs_recompile))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParamModifiedInfo {
    pub param_id: ParamID,
    pub new_value: Option<f64>,
    pub is_gesturing: bool,
}

pub(crate) enum OnIdleResult {
    Ok,
    PluginDeactivated,
    PluginActivated(PluginActivatedStatus),
    PluginReadyToRemove,
    PluginFailedToActivate(ActivatePluginError),
}
