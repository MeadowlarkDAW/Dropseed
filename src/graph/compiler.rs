use audio_graph::{
    AudioGraphHelper, InsertedDelay, InsertedSum, NodeID, PortID, ScheduleEntry, ScheduledNode,
};
use dropseed_plugin_api::buffer::{AudioPortBuffer, AudioPortBufferMut, SharedBuffer};
use dropseed_plugin_api::ext::audio_ports::{MainPortsLayout, PluginAudioPortsExt};
use dropseed_plugin_api::ext::note_ports::PluginNotePortsExt;
use dropseed_plugin_api::ProcBuffers;
use fnv::FnvHashMap;
use smallvec::{smallvec, SmallVec};

use crate::plugin_host::event_io_buffers::{NoteIoEvent, ParamIoEvent, PluginEventIoBuffers};
use crate::plugin_host::SharedPluginHostProcThread;
use crate::processor_schedule::tasks::{
    DeactivatedPluginTask, DelayCompNode, DelayCompTask, GraphInTask, GraphOutTask, PluginTask,
    SumTask, Task,
};

pub(super) mod verifier;

use verifier::Verifier;

use super::error::GraphCompilerError;
use super::shared_pools::{DelayCompKey, GraphSharedPools, SharedDelayCompNode};
use super::{ChannelID, PluginInstanceID, PortType, ProcessorSchedule};

fn schedule_graph_in_node(
    scheduled_node: &ScheduledNode,
    shared_pool: &mut GraphSharedPools,
    graph_in_id: &PluginInstanceID,
    num_graph_in_audio_ports: usize,
) -> Result<Task, GraphCompilerError> {
    // --- Construct a map that maps the index (channel) of each port to its assigned buffer

    let mut audio_out_slots: SmallVec<[Option<SharedBuffer<f32>>; 4]> =
        smallvec![None; num_graph_in_audio_ports];
    for output_buffer in scheduled_node.output_buffers.iter() {
        match output_buffer.type_index {
            PortType::AUDIO_TYPE_IDX => {
                let buffer = shared_pool
                    .buffers
                    .audio_buffer_pool
                    .initialized_buffer_at_index(output_buffer.buffer_index.0);

                let buffer_slot = audio_out_slots.get_mut(output_buffer.port_id.0 as usize).ok_or(
                    GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule assigned buffer to graph in node with invalid port id {:?}",
                    output_buffer
                )),
                )?;

                *buffer_slot = Some(buffer);
            }
            PortType::NOTE_TYPE_IDX => {
                // TODO: Note buffers in graph input.
            }
            _ => {
                return Err(GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule assigned buffer with invalid type index on graph in node {:?}",
                    output_buffer
                )));
            }
        }
    }

    // --- Construct the final task using the constructed map from above --------------------

    let mut audio_out: SmallVec<[SharedBuffer<f32>; 4]> =
        SmallVec::with_capacity(num_graph_in_audio_ports);
    for buffer_slot in audio_out_slots.drain(..) {
        let buffer = buffer_slot.ok_or(GraphCompilerError::UnexpectedError(format!(
            "Abstract schedule did not assign a buffer to all ports on graph in node {:?}",
            scheduled_node
        )))?;

        audio_out.push(buffer);
    }

    Ok(Task::GraphIn(GraphInTask { audio_out }))
}

fn schedule_graph_out_node(
    scheduled_node: &ScheduledNode,
    shared_pool: &mut GraphSharedPools,
    graph_out_id: &PluginInstanceID,
    num_graph_out_audio_ports: usize,
) -> Result<Task, GraphCompilerError> {
    // --- Construct a map that maps the index (channel) of each port to its assigned buffer

    let mut audio_in_slots: SmallVec<[Option<SharedBuffer<f32>>; 4]> =
        smallvec![None; num_graph_out_audio_ports];
    for input_buffer in scheduled_node.input_buffers.iter() {
        match input_buffer.type_index {
            PortType::AUDIO_TYPE_IDX => {
                let buffer = shared_pool
                    .buffers
                    .audio_buffer_pool
                    .initialized_buffer_at_index(input_buffer.buffer_index.0);

                let buffer_slot = audio_in_slots.get_mut(input_buffer.port_id.0 as usize).ok_or(
                    GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule assigned buffer to graph out node with invalid port id {:?}",
                    input_buffer
                )),
                )?;

                *buffer_slot = Some(buffer);
            }
            PortType::NOTE_TYPE_IDX => {
                // TODO: Note buffers in graph input.
            }
            _ => {
                return Err(GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule assigned buffer with invalid type index on graph out node {:?}",
                    input_buffer
                )));
            }
        }
    }

    // --- Construct the final task using the constructed map from above --------------------

    let mut audio_in: SmallVec<[SharedBuffer<f32>; 4]> =
        SmallVec::with_capacity(num_graph_out_audio_ports);
    for buffer_slot in audio_in_slots.drain(..) {
        let buffer = buffer_slot.ok_or(GraphCompilerError::UnexpectedError(format!(
            "Abstract schedule did not assign a buffer to all ports on graph out node {:?}",
            scheduled_node
        )))?;

        audio_in.push(buffer);
    }

    Ok(Task::GraphOut(GraphOutTask { audio_in }))
}

fn construct_deactivated_plugin_task(
    scheduled_node: &ScheduledNode,
    maybe_audio_ports_ext: Option<&PluginAudioPortsExt>,
    maybe_note_ports_ext: Option<&PluginNotePortsExt>,
    mut assigned_audio_buffers: FnvHashMap<ChannelID, (SharedBuffer<f32>, bool)>,
    mut assigned_note_buffers: FnvHashMap<ChannelID, (SharedBuffer<NoteIoEvent>, bool)>,
    assigned_param_event_out_buffer: Option<SharedBuffer<ParamIoEvent>>,
) -> Result<Task, GraphCompilerError> {
    // In this task, audio and note data is passed through the main ports (if the plugin
    // has main in/out ports), and then all the other output buffers are cleared.

    let mut audio_through: SmallVec<[(SharedBuffer<f32>, SharedBuffer<f32>); 4]> = SmallVec::new();
    let mut note_through: Option<(SharedBuffer<NoteIoEvent>, SharedBuffer<NoteIoEvent>)> = None;
    let mut clear_audio_out: SmallVec<[SharedBuffer<f32>; 4]> = SmallVec::new();
    let mut clear_note_out: SmallVec<[SharedBuffer<NoteIoEvent>; 2]> = SmallVec::new();
    let mut clear_param_event_out: Option<SharedBuffer<ParamIoEvent>> = None;

    if let Some(audio_ports_ext) = maybe_audio_ports_ext {
        if let MainPortsLayout::InOut = audio_ports_ext.main_ports_layout {
            let n_main_channels =
                audio_ports_ext.inputs[0].channels.min(audio_ports_ext.outputs[0].channels);

            for i in 0..n_main_channels {
                let in_channel_id = ChannelID {
                    stable_id: audio_ports_ext.inputs[0].stable_id,
                    port_type: PortType::Audio,
                    is_input: true,
                    channel: i,
                };

                let out_channel_id = ChannelID {
                    stable_id: audio_ports_ext.outputs[0].stable_id,
                    port_type: PortType::Audio,
                    is_input: false,
                    channel: i,
                };

                let in_buf = assigned_audio_buffers.get(&in_channel_id).ok_or(
                    GraphCompilerError::UnexpectedError(format!(
                        "Abstract schedule did not assign a buffer to every port in node {:?}",
                        scheduled_node
                    )),
                )?;
                let out_buf = assigned_audio_buffers.remove(&out_channel_id).ok_or(
                    GraphCompilerError::UnexpectedError(format!(
                        "Abstract schedule did not assign a buffer to every port in node {:?}",
                        scheduled_node
                    )),
                )?;

                audio_through.push((in_buf.0.clone(), out_buf.0));
            }
        }
    }

    if let Some(note_ports_ext) = maybe_note_ports_ext {
        if !note_ports_ext.inputs.is_empty() && !note_ports_ext.outputs.is_empty() {
            let in_channel_id = ChannelID {
                stable_id: note_ports_ext.inputs[0].stable_id,
                port_type: PortType::Note,
                is_input: true,
                channel: 0,
            };

            let out_channel_id = ChannelID {
                stable_id: note_ports_ext.outputs[0].stable_id,
                port_type: PortType::Note,
                is_input: false,
                channel: 0,
            };

            let in_buf = assigned_note_buffers.get(&in_channel_id).ok_or(
                GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule did not assign a buffer to every port in node {:?}",
                    scheduled_node
                )),
            )?;
            let out_buf = assigned_note_buffers.remove(&out_channel_id).ok_or(
                GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule did not assign a buffer to every port in node {:?}",
                    scheduled_node
                )),
            )?;

            note_through = Some((in_buf.0.clone(), out_buf.0));
        }
    }

    for (channel_id, (buffer, _)) in assigned_audio_buffers.iter() {
        if !channel_id.is_input {
            clear_audio_out.push(buffer.clone());
        }
    }
    for (channel_id, (buffer, _)) in assigned_note_buffers.iter() {
        if !channel_id.is_input {
            clear_note_out.push(buffer.clone());
        }
    }
    if let Some(buffer) = assigned_param_event_out_buffer {
        clear_param_event_out = Some(buffer.clone());
    }

    Ok(Task::DeactivatedPlugin(DeactivatedPluginTask {
        audio_through,
        note_through,
        clear_audio_out,
        clear_note_out,
        clear_param_event_out,
    }))
}

fn construct_plugin_task(
    scheduled_node: &ScheduledNode,
    shared_pool: &GraphSharedPools,
    plugin_id: PluginInstanceID,
    shared_processor: &SharedPluginHostProcThread,
    audio_ports_ext: &PluginAudioPortsExt,
    note_ports_ext: &PluginNotePortsExt,
    mut assigned_audio_buffers: FnvHashMap<ChannelID, (SharedBuffer<f32>, bool)>,
    mut assigned_note_buffers: FnvHashMap<ChannelID, (SharedBuffer<NoteIoEvent>, bool)>,
    assigned_param_event_in_buffer: Option<(SharedBuffer<ParamIoEvent>, bool)>,
    assigned_param_event_out_buffer: Option<SharedBuffer<ParamIoEvent>>,
) -> Result<Task, GraphCompilerError> {
    let mut audio_in: SmallVec<[AudioPortBuffer; 2]> = SmallVec::new();
    let mut audio_out: SmallVec<[AudioPortBufferMut; 2]> = SmallVec::new();
    let mut note_in_buffers: SmallVec<[SharedBuffer<NoteIoEvent>; 2]> = SmallVec::new();
    let mut note_out_buffers: SmallVec<[SharedBuffer<NoteIoEvent>; 2]> = SmallVec::new();
    let mut clear_audio_in_buffers: SmallVec<[SharedBuffer<f32>; 2]> = SmallVec::new();
    let mut clear_note_in_buffers: SmallVec<[SharedBuffer<NoteIoEvent>; 2]> = SmallVec::new();

    for in_port in audio_ports_ext.inputs.iter() {
        let mut buffers: SmallVec<[SharedBuffer<f32>; 2]> =
            SmallVec::with_capacity(usize::from(in_port.channels));
        for channel_i in 0..in_port.channels {
            let channel_id = ChannelID {
                stable_id: in_port.stable_id,
                port_type: PortType::Audio,
                is_input: true,
                channel: channel_i,
            };

            let buffer = assigned_audio_buffers.get(&channel_id).ok_or(
                GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule did not assign a buffer to every port in node {:?}",
                    scheduled_node
                )),
            )?;

            buffers.push(buffer.0.clone());

            if buffer.1 {
                clear_audio_in_buffers.push(buffer.0.clone());
            }
        }

        audio_in.push(AudioPortBuffer::_new(
            buffers,
            shared_pool.buffers.audio_buffer_pool.buffer_size() as u32,
        ));
        // TODO: proper latency?
    }
    for out_port in audio_ports_ext.outputs.iter() {
        let mut buffers: SmallVec<[SharedBuffer<f32>; 2]> =
            SmallVec::with_capacity(usize::from(out_port.channels));
        for channel_i in 0..out_port.channels {
            let channel_id = ChannelID {
                stable_id: out_port.stable_id,
                port_type: PortType::Audio,
                is_input: false,
                channel: channel_i,
            };

            let buffer = assigned_audio_buffers.get(&channel_id).ok_or(
                GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule did not assign a buffer to every port in node {:?}",
                    scheduled_node
                )),
            )?;

            buffers.push(buffer.0.clone());
        }

        audio_out.push(AudioPortBufferMut::_new(
            buffers,
            shared_pool.buffers.audio_buffer_pool.buffer_size() as u32,
        ));
        // TODO: proper latency?
    }

    for in_port in note_ports_ext.inputs.iter() {
        let channel_id = ChannelID {
            stable_id: in_port.stable_id,
            port_type: PortType::Note,
            is_input: true,
            channel: 0,
        };

        let buffer = assigned_note_buffers.get(&channel_id).ok_or(
            GraphCompilerError::UnexpectedError(format!(
                "Abstract schedule did not assign a buffer to every port in node {:?}",
                scheduled_node
            )),
        )?;

        note_in_buffers.push(buffer.0.clone());

        if buffer.1 {
            clear_note_in_buffers.push(buffer.0.clone());
        }
    }
    for out_port in note_ports_ext.outputs.iter() {
        let channel_id = ChannelID {
            stable_id: out_port.stable_id,
            port_type: PortType::Note,
            is_input: false,
            channel: 0,
        };

        let buffer = assigned_note_buffers.get(&channel_id).ok_or(
            GraphCompilerError::UnexpectedError(format!(
                "Abstract schedule did not assign a buffer to every port in node {:?}",
                scheduled_node
            )),
        )?;

        note_out_buffers.push(buffer.0.clone());
    }

    Ok(Task::Plugin(PluginTask {
        plugin_id,
        shared_processor: shared_processor.clone(),
        buffers: ProcBuffers { audio_in, audio_out },
        event_buffers: PluginEventIoBuffers {
            note_in_buffers,
            note_out_buffers,
            clear_note_in_buffers,
            param_event_in_buffer: assigned_param_event_in_buffer,
            param_event_out_buffer: assigned_param_event_out_buffer,
        },
        clear_audio_in_buffers,
    }))
}

fn schedule_node(
    scheduled_node: &ScheduledNode,
    shared_pool: &mut GraphSharedPools,
    graph_in_id: &PluginInstanceID,
    graph_out_id: &PluginInstanceID,
    num_graph_in_audio_ports: usize,
    num_graph_out_audio_ports: usize,
) -> Result<Task, GraphCompilerError> {
    // Special case for the graph in/out nodes
    if scheduled_node.id.0 == graph_in_id._node_id() {
        return schedule_graph_in_node(
            scheduled_node,
            shared_pool,
            graph_in_id,
            num_graph_in_audio_ports,
        );
    } else if scheduled_node.id.0 == graph_out_id._node_id() {
        return schedule_graph_out_node(
            scheduled_node,
            shared_pool,
            graph_out_id,
            num_graph_out_audio_ports,
        );
    }

    // --- Get port info and processor from the plugin host ---------------------------------

    let plugin_host = shared_pool.plugin_hosts.pool.get(&scheduled_node.id).ok_or(
        GraphCompilerError::UnexpectedError(format!(
            "Abstract schedule assigned a node that doesn't exist: {:?}",
            scheduled_node
        )),
    )?;

    let plugin_id = plugin_host.id();
    let port_ids = plugin_host.port_ids();
    let num_audio_in_channels = plugin_host.num_audio_in_channels();
    let num_audio_out_channels = plugin_host.num_audio_out_channels();
    let maybe_shared_processor = plugin_host.shared_processor();
    let maybe_audio_ports_ext = plugin_host.audio_ports_ext();
    let maybe_note_ports_ext = plugin_host.note_ports_ext();

    // --- Construct a map that maps the ChannelID of each port to its assigned buffer ------

    let mut assigned_audio_buffers: FnvHashMap<ChannelID, (SharedBuffer<f32>, bool)> =
        FnvHashMap::default();
    let mut assigned_note_buffers: FnvHashMap<ChannelID, (SharedBuffer<NoteIoEvent>, bool)> =
        FnvHashMap::default();
    let mut assigned_param_event_in_buffer: Option<(SharedBuffer<ParamIoEvent>, bool)> = None;
    let mut assigned_param_event_out_buffer: Option<SharedBuffer<ParamIoEvent>> = None;

    for assigned_buffer in
        scheduled_node.input_buffers.iter().chain(scheduled_node.output_buffers.iter())
    {
        let channel_id = port_ids.port_id_to_channel_id.get(&assigned_buffer.port_id).ok_or(
            GraphCompilerError::UnexpectedError(format!(
                "Abstract schedule assigned a buffer for port that doesn't exist {:?}",
                scheduled_node
            )),
        )?;

        if assigned_buffer.type_index != channel_id.port_type.as_type_idx() {
            return Err(GraphCompilerError::UnexpectedError(format!(
                "Abstract schedule assigned the wrong type of buffer for port {:?}",
                scheduled_node
            )));
        }

        match channel_id.port_type {
            PortType::Audio => {
                let buffer = shared_pool
                    .buffers
                    .audio_buffer_pool
                    .initialized_buffer_at_index(assigned_buffer.buffer_index.0);

                if assigned_audio_buffers
                    .insert(*channel_id, (buffer, assigned_buffer.should_clear))
                    .is_some()
                {
                    return Err(GraphCompilerError::UnexpectedError(format!(
                        "Abstract schedule assigned multiple buffers to the same port {:?}",
                        scheduled_node
                    )));
                }
            }
            PortType::Note => {
                let buffer = shared_pool
                    .buffers
                    .note_buffer_pool
                    .buffer_at_index(assigned_buffer.buffer_index.0);

                if assigned_note_buffers
                    .insert(*channel_id, (buffer, assigned_buffer.should_clear))
                    .is_some()
                {
                    return Err(GraphCompilerError::UnexpectedError(format!(
                        "Abstract schedule assigned multiple buffers to the same port {:?}",
                        scheduled_node
                    )));
                }
            }
            PortType::ParamAutomation => {
                let buffer = shared_pool
                    .buffers
                    .param_event_buffer_pool
                    .buffer_at_index(assigned_buffer.buffer_index.0);

                if channel_id.is_input {
                    if assigned_param_event_in_buffer.is_some() {
                        return Err(GraphCompilerError::UnexpectedError(format!(
                            "Abstract schedule assigned multiple buffers to the param automation in port {:?}",
                            scheduled_node
                        )));
                    }
                    assigned_param_event_in_buffer = Some((buffer, assigned_buffer.should_clear));
                } else {
                    if assigned_param_event_out_buffer.is_some() {
                        return Err(GraphCompilerError::UnexpectedError(format!(
                            "Abstract schedule assigned multiple buffers to the param automation out port {:?}",
                            scheduled_node
                        )));
                    }
                    assigned_param_event_out_buffer = Some(buffer);
                }
            }
        }
    }

    // --- Construct the final task using the constructed map from above --------------------

    if maybe_shared_processor.is_none() {
        // Plugin is unloaded/deactivated
        construct_deactivated_plugin_task(
            scheduled_node,
            maybe_audio_ports_ext,
            maybe_note_ports_ext,
            assigned_audio_buffers,
            assigned_note_buffers,
            assigned_param_event_out_buffer,
        )
    } else {
        construct_plugin_task(
            scheduled_node,
            shared_pool,
            *plugin_id,
            maybe_shared_processor.as_ref().unwrap(),
            maybe_audio_ports_ext.as_ref().unwrap(),
            maybe_note_ports_ext.as_ref().unwrap(),
            assigned_audio_buffers,
            assigned_note_buffers,
            assigned_param_event_in_buffer,
            assigned_param_event_out_buffer,
        )
    }
}

pub(super) fn compile_graph(
    shared_pool: &mut GraphSharedPools,
    graph_helper: &mut AudioGraphHelper,
    graph_in_id: &PluginInstanceID,
    graph_out_id: &PluginInstanceID,
    num_graph_in_audio_ports: usize,
    num_graph_out_audio_ports: usize,
    verifier: &mut Verifier,
    coll_handle: &basedrop::Handle,
) -> Result<ProcessorSchedule, GraphCompilerError> {
    let num_plugins = shared_pool.plugin_hosts.pool.len();

    let mut tasks: Vec<Task> = Vec::with_capacity(num_plugins * 2);

    let mut graph_audio_in: SmallVec<[SharedBuffer<f32>; 4]> = SmallVec::new();
    let mut graph_audio_out: SmallVec<[SharedBuffer<f32>; 4]> = SmallVec::new();

    for node in shared_pool.delay_comp_nodes.pool.values_mut() {
        node.active = false;
    }

    let abstract_schedule = graph_helper.compile()?;

    shared_pool.buffers.set_num_buffers(
        abstract_schedule.num_buffers[PortType::Audio.into()],
        abstract_schedule.num_buffers[PortType::Note.into()],
        abstract_schedule.num_buffers[PortType::ParamAutomation.into()],
    );

    for schedule_entry in abstract_schedule.schedule.iter() {
        match schedule_entry {
            ScheduleEntry::Node(scheduled_node) => {
                schedule_node(
                    scheduled_node,
                    shared_pool,
                    graph_in_id,
                    graph_out_id,
                    num_graph_in_audio_ports,
                    num_graph_out_audio_ports,
                )?;
            }
            ScheduleEntry::Delay(inserted_delay) => {}
            ScheduleEntry::Sum(inserted_sum) => {}
        }
    }

    let new_schedule = ProcessorSchedule::new(
        tasks,
        shared_pool.transports.transport.clone(),
        shared_pool.buffers.audio_buffer_pool.buffer_size(),
    );

    // This is probably expensive, but I would like to keep this check here until we are very
    // confident in the stability and soundness of this audio graph compiler.
    //
    // We are using reference-counted pointers (`basedrop::Shared`) for everything, so we shouldn't
    // ever run into a situation where the schedule assigns a pointer to a buffer or a node that
    // doesn't exist in memory.
    //
    // However, it is still very possible to have race condition bugs in the schedule, such as
    // the same buffer being assigned multiple times within the same task, or the same buffer
    // appearing multiple times between parallel tasks (once we have multithreaded scheduling).
    if let Err(e) = verifier.verify_schedule_for_race_conditions(&new_schedule) {
        return Err(GraphCompilerError::VerifierError(e, abstract_tasks.to_vec(), new_schedule));
    }

    shared_pool.buffers.remove_excess_buffers(
        max_graph_audio_buffer_index,
        total_intermediary_buffers,
        max_graph_note_buffer_index,
        max_graph_automation_buffer_index,
    );

    // TODO: Optimize this?
    //
    // Not stable yet apparently
    // plugin_pool.delay_comp_nodes.drain_filter(|_, node| !node.active);
    shared_pool.delay_comp_nodes.pool =
        shared_pool.delay_comp_nodes.pool.drain().filter(|(_, node)| node.active).collect();

    Ok(new_schedule)
}
