use audio_graph::{Graph, NodeRef};
use basedrop::Shared;
use crossbeam_channel::Sender;
use dropseed_plugin_api::ext::audio_ports::{MainPortsLayout, PluginAudioPortsExt};
use dropseed_plugin_api::ext::note_ports::PluginNotePortsExt;
use dropseed_plugin_api::ext::params::{ParamID, ParamInfo};
use dropseed_plugin_api::transport::TempoMap;
use dropseed_plugin_api::{
    DSPluginSaveState, HostRequestChannelReceiver, HostRequestFlags, PluginInstanceID,
    PluginMainThread,
};
use fnv::FnvHashMap;
use meadowlark_core_types::time::SampleRate;
use smallvec::SmallVec;

use crate::engine::events::from_engine::{DSEngineEvent, PluginEvent};
use crate::graph::{PortChannelID, PortType};

use super::channel::{PlugHostChannelMainThread, PluginActiveState, SharedPluginHostProcThread};
use super::error::ActivatePluginError;

/// The references to this plugin's ports in the audio graph.
pub(crate) struct PluginHostPortRefs {
    pub port_channels_refs: FnvHashMap<PortChannelID, audio_graph::PortRef>,
    pub main_audio_in_port_refs: Vec<audio_graph::PortRef>,
    pub main_audio_out_port_refs: Vec<audio_graph::PortRef>,
    pub automation_in_port_ref: Option<audio_graph::PortRef>,
    pub automation_out_port_ref: Option<audio_graph::PortRef>,
    pub main_note_in_port_ref: Option<audio_graph::PortRef>,
    pub main_note_out_port_ref: Option<audio_graph::PortRef>,
}

impl PluginHostPortRefs {
    pub fn new() -> Self {
        Self {
            port_channels_refs: FnvHashMap::default(),
            main_audio_in_port_refs: Vec::new(),
            main_audio_out_port_refs: Vec::new(),
            automation_in_port_ref: None,
            automation_out_port_ref: None,
            main_note_in_port_ref: None,
            main_note_out_port_ref: None,
        }
    }
}

pub(crate) struct PluginHostMainThread {
    id: PluginInstanceID,

    audio_ports_ext: Option<PluginAudioPortsExt>,
    note_ports_ext: Option<PluginNotePortsExt>,

    num_audio_in_channels: usize,
    num_audio_out_channels: usize,

    plug_main_thread: Box<dyn PluginMainThread>,

    port_refs: PluginHostPortRefs,

    channel: PlugHostChannelMainThread,

    save_state: DSPluginSaveState,

    gesturing_params: FnvHashMap<ParamID, bool>,

    host_request_rx: HostRequestChannelReceiver,
    remove_requested: bool,
    save_state_dirty: bool,
    restarting: bool,
}

impl PluginHostMainThread {
    pub fn new(
        id: PluginInstanceID,
        save_state: DSPluginSaveState,
        mut plug_main_thread: Box<dyn PluginMainThread>,
        host_request_rx: HostRequestChannelReceiver,
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
            if let Some(backup_audio_ports) = &save_state.backup_audio_ports {
                (backup_audio_ports.total_in_channels(), backup_audio_ports.total_out_channels())
            } else {
                (0, 0)
            };

        Self {
            id,
            plug_main_thread,
            port_refs: PluginHostPortRefs::new(),
            audio_ports_ext: None,
            note_ports_ext: None,
            num_audio_in_channels,
            num_audio_out_channels,
            channel: PlugHostChannelMainThread::new(),
            save_state,
            gesturing_params: FnvHashMap::default(),
            host_request_rx,
            remove_requested: false,
            save_state_dirty: false,
            restarting: false,
        }
    }

    pub fn load_save_state(&mut self, state: Vec<u8>) {
        match self.plug_main_thread.load_save_state(state) {
            Ok(()) => {
                log::trace!("Plugin {:?} successfully loaded state", &self.id);
            }
            Err(e) => {
                log::error!("Plugin {:?} failed to load state: {}", &self.id, e);
            }
        }

        self.save_state_dirty = true;
    }

    pub fn is_save_state_dirty(&self) -> bool {
        self.save_state_dirty
    }

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

    pub fn show_gui(&mut self) {
        if !self.plug_main_thread.is_gui_open() {
            if let Err(e) = self.plug_main_thread.open_gui(None) {
                log::warn!("Could not open GUI for plugin {:?}: {:?}", &self.id, e);
            }
        }
    }

    pub fn close_gui(&mut self) {
        if self.plug_main_thread.is_gui_open() {
            self.plug_main_thread.close_gui()
        }
    }

    pub fn supports_gui(&self) -> bool {
        self.plug_main_thread.supports_gui()
    }

    pub fn can_activate(&self) -> Result<(), ActivatePluginError> {
        // TODO: without this check it seems something is attempting to activate the plugin twice
        if self.channel.shared_state.get_active_state() == PluginActiveState::Active {
            return Err(ActivatePluginError::AlreadyActive);
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
        (FnvHashMap<ParamID, f64>, Option<PluginAudioPortsExt>, Option<PluginNotePortsExt>),
        ActivatePluginError,
    > {
        self.can_activate()?;

        let audio_ports = match self.plug_main_thread.audio_ports_ext() {
            Ok(audio_ports) => {
                self.num_audio_in_channels = audio_ports.total_in_channels();
                self.num_audio_out_channels = audio_ports.total_out_channels();

                self.save_state.backup_audio_ports = Some(audio_ports.clone());

                audio_ports
            }
            Err(e) => {
                self.channel.shared_state.set_active_state(PluginActiveState::InactiveWithError);
                self.audio_ports_ext = None;

                return Err(ActivatePluginError::PluginFailedToGetAudioPortsExt(e));
            }
        };

        let note_ports = match self.plug_main_thread.note_ports_ext() {
            Ok(note_ports) => {
                self.save_state.backup_note_ports = Some(note_ports.clone());

                note_ports
            }
            Err(e) => {
                self.channel.shared_state.set_active_state(PluginActiveState::InactiveWithError);
                self.note_ports_ext = None;

                return Err(ActivatePluginError::PluginFailedToGetNotePortsExt(e));
            }
        };

        self.audio_ports_ext = Some(audio_ports.clone());
        self.note_ports_ext = Some(note_ports.clone());

        let num_params = self.plug_main_thread.num_params() as usize;
        let mut params: FnvHashMap<ParamID, ParamInfo> = FnvHashMap::default();
        let mut param_values: FnvHashMap<ParamID, f64> = FnvHashMap::default();

        for i in 0..num_params {
            match self.plug_main_thread.param_info(i) {
                Ok(info) => match self.plug_main_thread.param_value(info.stable_id) {
                    Ok(value) => {
                        let id = info.stable_id;

                        let _ = params.insert(id, info);
                        let _ = param_values.insert(id, value);
                    }
                    Err(_) => {
                        self.channel
                            .shared_state
                            .set_active_state(PluginActiveState::InactiveWithError);

                        return Err(ActivatePluginError::PluginFailedToGetParamValue(
                            info.stable_id,
                        ));
                    }
                },
                Err(_) => {
                    self.channel
                        .shared_state
                        .set_active_state(PluginActiveState::InactiveWithError);

                    return Err(ActivatePluginError::PluginFailedToGetParamInfo(i));
                }
            }
        }

        match self.plug_main_thread.activate(sample_rate, min_frames, max_frames, coll_handle) {
            Ok(info) => {
                self.channel.shared_state.set_active_state(PluginActiveState::Active);

                self.channel.create_process_thread(info.processor, num_params, coll_handle);

                Ok((
                    param_values,
                    // TODO: Only return the new extensions if they have changed.
                    Some(self.audio_ports_ext.as_ref().unwrap().clone()),
                    Some(self.note_ports_ext.as_ref().unwrap().clone()),
                ))
            }
            Err(e) => {
                self.channel.shared_state.set_active_state(PluginActiveState::InactiveWithError);

                Err(ActivatePluginError::PluginSpecific(e))
            }
        }
    }

    pub fn schedule_deactivate(&mut self) {
        if self.channel.shared_state.get_active_state() != PluginActiveState::Active {
            return;
        }

        // Allow the plugin's audio thread to be dropped when the new
        // schedule is sent.
        //
        // Note this doesn't actually drop the process thread. It only
        // drops this struct's pointer to the process thread.
        self.channel.drop_process_thread_pointer();

        // Wait for the audio thread part to go to sleep before
        // deactivating.
        self.channel.shared_state.set_active_state(PluginActiveState::WaitingToDrop);
    }

    pub fn schedule_remove(&mut self) {
        self.remove_requested = true;

        self.schedule_deactivate();
    }

    pub fn audio_ports_ext(&self) -> Option<&PluginAudioPortsExt> {
        if self.audio_ports_ext.is_some() {
            self.audio_ports_ext.as_ref()
        } else {
            self.save_state.backup_audio_ports.as_ref()
        }
    }

    pub fn note_ports_ext(&self) -> Option<&PluginNotePortsExt> {
        if self.note_ports_ext.is_some() {
            self.note_ports_ext.as_ref()
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
        event_tx: &mut Option<&mut Sender<DSEngineEvent>>,
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

        if request_flags.contains(HostRequestFlags::RESTART) && !self.remove_requested {
            self.restarting = true;
            if active_state != PluginActiveState::DroppedAndReadyToDeactivate {
                self.channel.shared_state.set_active_state(PluginActiveState::WaitingToDrop);
                active_state = PluginActiveState::WaitingToDrop;
            }
        }

        if request_flags.intersects(HostRequestFlags::GUI_CLOSED | HostRequestFlags::GUI_DESTROYED)
        {
            self.plug_main_thread
                .on_gui_closed(request_flags.contains(HostRequestFlags::GUI_DESTROYED));

            if let Some(event_tx) = event_tx.as_mut() {
                event_tx
                    .send(DSEngineEvent::Plugin(PluginEvent::GuiClosed {
                        plugin_id: self.id.clone(),
                    }))
                    .unwrap()
            }
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

            self.channel.drop_process_thread_pointer();
            self.save_state_dirty = true;

            if !self.remove_requested {
                let mut res = OnIdleResult::PluginDeactivated;

                if self.restarting || request_flags.contains(HostRequestFlags::PROCESS) {
                    match self.activate(sample_rate, min_frames, max_frames, coll_handle) {
                        Ok(r) => {
                            res = OnIdleResult::PluginActivated {
                                new_param_values: r.0,
                                new_audio_ports: r.1,
                                new_note_ports: r.2,
                            };
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
                let res = match self.activate(sample_rate, min_frames, max_frames, coll_handle) {
                    Ok(r) => {
                        self.save_state_dirty = true;

                        OnIdleResult::PluginActivated {
                            new_param_values: r.0,
                            new_audio_ports: r.1,
                            new_note_ports: r.2,
                        }
                    }
                    Err(e) => OnIdleResult::PluginFailedToActivate(e),
                };

                return (res, modified_params);
            }
        }

        (OnIdleResult::Ok, modified_params)
    }

    pub fn update_tempo_map(&mut self, new_tempo_map: &Shared<TempoMap>) {
        self.plug_main_thread.update_tempo_map(new_tempo_map);
    }

    pub fn num_audio_in_channels(&self) -> usize {
        self.num_audio_in_channels
    }

    pub fn num_audio_out_channels(&self) -> usize {
        self.num_audio_out_channels
    }

    pub fn shared_processor(&self) -> &Option<SharedPluginHostProcThread> {
        self.channel.shared_processor()
    }

    pub fn id(&self) -> &PluginInstanceID {
        &self.id
    }

    pub fn port_refs(&self) -> &PluginHostPortRefs {
        &self.port_refs
    }

    pub fn sync_ports_in_graph(
        &mut self,
        abstract_graph: &mut Graph<PluginInstanceID, PortChannelID, PortType>,
    ) {
        let mut prev_port_channel_refs = self.port_refs.port_channels_refs.clone();
        self.port_refs.port_channels_refs.clear();

        self.port_refs.main_audio_in_port_refs.clear();
        self.port_refs.main_audio_out_port_refs.clear();
        self.port_refs.main_note_in_port_ref = None;
        self.port_refs.main_note_out_port_ref = None;

        if let Some(audio_ports) = &self.audio_ports_ext {
            for (audio_port_i, audio_in_port) in audio_ports.inputs.iter().enumerate() {
                for i in 0..audio_in_port.channels {
                    let port_id = PortChannelID {
                        port_type: PortType::Audio,
                        port_stable_id: audio_in_port.stable_id,
                        is_input: true,
                        port_channel: i,
                    };

                    let port_ref = if let Some(port_ref) = prev_port_channel_refs.get(&port_id) {
                        let port_ref = *port_ref;
                        let _ = prev_port_channel_refs.remove(&port_id);
                        port_ref
                    } else {
                        let port_ref = abstract_graph
                            .port(NodeRef::new(self.id._node_ref()), PortType::Audio, port_id)
                            .unwrap();

                        let _ = self.port_refs.port_channels_refs.insert(port_id, port_ref);

                        port_ref
                    };

                    if audio_port_i == 0 {
                        match audio_ports.main_ports_layout {
                            MainPortsLayout::InOut | MainPortsLayout::InOnly => {
                                self.port_refs.main_audio_in_port_refs.push(port_ref);
                            }
                            _ => {}
                        }
                    }
                }
            }

            for (audio_port_i, audio_out_port) in audio_ports.outputs.iter().enumerate() {
                for i in 0..audio_out_port.channels {
                    let port_id = PortChannelID {
                        port_type: PortType::Audio,
                        port_stable_id: audio_out_port.stable_id,
                        is_input: false,
                        port_channel: i,
                    };

                    let port_ref = if let Some(port_ref) = prev_port_channel_refs.get(&port_id) {
                        let port_ref = *port_ref;
                        let _ = prev_port_channel_refs.remove(&port_id);
                        port_ref
                    } else {
                        let port_ref = abstract_graph
                            .port(NodeRef::new(self.id._node_ref()), PortType::Audio, port_id)
                            .unwrap();

                        let _ = self.port_refs.port_channels_refs.insert(port_id, port_ref);

                        port_ref
                    };

                    if audio_port_i == 0 {
                        match audio_ports.main_ports_layout {
                            MainPortsLayout::InOut | MainPortsLayout::OutOnly => {
                                self.port_refs.main_audio_out_port_refs.push(port_ref);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        const IN_AUTOMATION_PORT_ID: PortChannelID = PortChannelID {
            port_type: PortType::ParamAutomation,
            port_stable_id: 0,
            is_input: true,
            port_channel: 0,
        };
        const OUT_AUTOMATION_PORT_ID: PortChannelID = PortChannelID {
            port_type: PortType::ParamAutomation,
            port_stable_id: 1,
            is_input: false,
            port_channel: 0,
        };

        // Plugins always have one automation in port.
        if prev_port_channel_refs.get(&IN_AUTOMATION_PORT_ID).is_none() {
            let in_port_ref = abstract_graph
                .port(
                    NodeRef::new(self.id._node_ref()),
                    PortType::ParamAutomation,
                    IN_AUTOMATION_PORT_ID,
                )
                .unwrap();

            let _ = self.port_refs.port_channels_refs.insert(IN_AUTOMATION_PORT_ID, in_port_ref);

            self.port_refs.automation_in_port_ref = Some(in_port_ref);
        } else {
            let _ = prev_port_channel_refs.remove(&IN_AUTOMATION_PORT_ID);
        }

        if self.plug_main_thread.has_automation_out_port() {
            if prev_port_channel_refs.get(&OUT_AUTOMATION_PORT_ID).is_none() {
                let out_port_ref = abstract_graph
                    .port(
                        NodeRef::new(self.id._node_ref()),
                        PortType::ParamAutomation,
                        OUT_AUTOMATION_PORT_ID,
                    )
                    .unwrap();

                let _ =
                    self.port_refs.port_channels_refs.insert(OUT_AUTOMATION_PORT_ID, out_port_ref);

                self.port_refs.automation_out_port_ref = Some(out_port_ref);
            } else {
                let _ = prev_port_channel_refs.remove(&OUT_AUTOMATION_PORT_ID);
            }
        } else {
            self.port_refs.automation_out_port_ref = None;
        }

        if let Some(note_ports) = &self.note_ports_ext {
            for (i, note_in_port) in note_ports.inputs.iter().enumerate() {
                let port_id = PortChannelID {
                    port_type: PortType::Note,
                    port_stable_id: note_in_port.stable_id,
                    is_input: true,
                    port_channel: 0,
                };

                let port_ref = if let Some(port_ref) = prev_port_channel_refs.get(&port_id) {
                    let port_ref = *port_ref;
                    let _ = prev_port_channel_refs.remove(&port_id);

                    port_ref
                } else {
                    let port_ref = abstract_graph
                        .port(NodeRef::new(self.id._node_ref()), PortType::Note, port_id)
                        .unwrap();

                    let _ = self.port_refs.port_channels_refs.insert(port_id, port_ref);

                    port_ref
                };

                if i == 0 {
                    self.port_refs.main_note_in_port_ref = Some(port_ref);
                }
            }

            for (i, note_out_port) in note_ports.outputs.iter().enumerate() {
                let port_id = PortChannelID {
                    port_type: PortType::Note,
                    port_stable_id: note_out_port.stable_id,
                    is_input: false,
                    port_channel: 0,
                };

                let port_ref = if let Some(port_ref) = prev_port_channel_refs.get(&port_id) {
                    let port_ref = *port_ref;
                    let _ = prev_port_channel_refs.remove(&port_id);

                    port_ref
                } else {
                    let port_ref = abstract_graph
                        .port(NodeRef::new(self.id._node_ref()), PortType::Note, port_id)
                        .unwrap();

                    let _ = self.port_refs.port_channels_refs.insert(port_id, port_ref);

                    port_ref
                };

                if i == 0 {
                    self.port_refs.main_note_out_port_ref = Some(port_ref);
                }
            }
        }

        for (_, removed_port) in prev_port_channel_refs.drain() {
            abstract_graph.delete_port(removed_port).unwrap();
        }
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
    PluginActivated {
        new_param_values: FnvHashMap<ParamID, f64>,
        new_audio_ports: Option<PluginAudioPortsExt>,
        new_note_ports: Option<PluginNotePortsExt>,
    },
    PluginReadyToRemove,
    PluginFailedToActivate(ActivatePluginError),
}
