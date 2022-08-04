use audio_graph::{Graph, ScheduledNode};
use dropseed_plugin_api::buffer::{AudioPortBuffer, AudioPortBufferMut, SharedBuffer};
use dropseed_plugin_api::ext::audio_ports::MainPortsLayout;
use dropseed_plugin_api::ProcBuffers;
use smallvec::SmallVec;
use std::error::Error;

use crate::plugin_host::events::PluginEventIoBuffers;
use crate::schedule::tasks::{
    DeactivatedPluginTask, DelayCompNode, DelayCompTask, PluginTask, SumTask, Task,
};

pub(super) mod verifier;

use verifier::Verifier;

pub use verifier::VerifyScheduleError;

use super::shared_pools::{DelayCompKey, GraphSharedPools, SharedDelayCompNode};
use super::{PluginInstanceID, PortChannelID, PortType, Schedule};

pub(crate) fn compile_graph(
    shared_pool: &mut GraphSharedPools,
    abstract_graph: &mut Graph<PluginInstanceID, PortChannelID, PortType>,
    graph_in_node_id: &PluginInstanceID,
    graph_out_node_id: &PluginInstanceID,
    verifier: &mut Verifier,
    coll_handle: &basedrop::Handle,
) -> Result<Schedule, GraphCompilerError> {
    let num_plugins = shared_pool.plugin_hosts.pool.len();

    let mut tasks: Vec<Task> = Vec::with_capacity(num_plugins * 2);

    let mut graph_audio_in: SmallVec<[SharedBuffer<f32>; 4]> = SmallVec::new();
    let mut graph_audio_out: SmallVec<[SharedBuffer<f32>; 4]> = SmallVec::new();

    let mut total_intermediary_buffers = 0;
    let mut max_graph_audio_buffer_index = 0;
    let mut max_graph_note_buffer_index = 0;
    let mut max_graph_automation_buffer_index = 0;

    for node in shared_pool.delay_comp_nodes.pool.values_mut() {
        node.active = false;
    }

    let abstract_tasks = abstract_graph.compile();
    for abstract_task in abstract_tasks.iter() {
        let (
            plugin_id,
            num_audio_in_channels,
            num_audio_out_channels,
            shared_processor,
            audio_ports_ext,
            note_ports_ext,
        ) = if let Some(plugin_host) = shared_pool.plugin_hosts.pool.get(&abstract_task.node) {
            (
                plugin_host.id(),
                plugin_host.num_audio_in_channels(),
                plugin_host.num_audio_out_channels(),
                plugin_host.shared_processor(),
                plugin_host.audio_ports_ext(),
                plugin_host.note_ports_ext(),
            )
        } else {
            return Err(GraphCompilerError::UnexpectedError(format!(
                "Abstract schedule refers to a plugin instance {:?} that does not exist in the plugin pool",
                abstract_task.node
            ), abstract_tasks.to_vec()));
        };

        let mut intermediary_buffer_i = 0;

        let mut plugin_in_channel_buffers: SmallVec<[Option<SharedBuffer<f32>>; 4]> =
            SmallVec::from_elem(None, num_audio_in_channels);
        let mut plugin_out_channel_buffers: SmallVec<[Option<SharedBuffer<f32>>; 4]> =
            SmallVec::from_elem(None, num_audio_out_channels);

        let (num_note_in_ports, num_note_out_ports) = if let Some(note_ports_ext) = note_ports_ext {
            (note_ports_ext.inputs.len(), note_ports_ext.outputs.len())
        } else {
            (0, 0)
        };

        let mut note_in_buffers = SmallVec::from_elem(None, num_note_in_ports);
        let mut note_out_buffers = SmallVec::from_elem(None, num_note_out_ports);

        let mut automation_in_buffers = None;
        let mut automation_out_buffer = None;

        let mut next_audio_in_channel_index = 0;
        let mut next_audio_out_channel_index = 0;

        for (port_channel_id, buffers) in abstract_task.inputs.iter() {
            match port_channel_id.port_type {
                PortType::Audio => {
                    let channel_index = if let Some(audio_ports_ext) = audio_ports_ext {
                        match audio_ports_ext.in_channel_index(
                            port_channel_id.port_stable_id,
                            port_channel_id.port_channel,
                        ) {
                            Some(index) => index,
                            None => {
                                return Err(GraphCompilerError::UnexpectedError(format!(
                                    "Abstract schedule refers to an input port with ID {} and channel {} in the plugin {:?} that doesn't exist",
                                    port_channel_id.port_stable_id,
                                    port_channel_id.port_channel,
                                    abstract_task.node,
                                ), abstract_tasks.to_vec()));
                            }
                        }
                    } else {
                        next_audio_in_channel_index += 1;
                        next_audio_in_channel_index - 1
                    };

                    let mut channel_buffers: SmallVec<[SharedBuffer<f32>; 4]> = SmallVec::new();
                    for (buffer, delay_comp_info) in buffers.iter() {
                        max_graph_audio_buffer_index =
                            max_graph_audio_buffer_index.max(buffer.buffer_id);

                        let graph_buffer = shared_pool
                            .buffers
                            .audio_buffer_pool
                            .initialized_buffer_at_index(buffer.buffer_id);

                        let channel_buffer = if let Some(delay_comp_info) = &delay_comp_info {
                            // Add an intermediate buffer for the delay compensation task.
                            let intermediary_buffer = shared_pool
                                .buffers
                                .intermediary_audio_buffer_pool
                                .initialized_buffer_at_index(intermediary_buffer_i);

                            intermediary_buffer_i += 1;
                            total_intermediary_buffers =
                                total_intermediary_buffers.max(intermediary_buffer_i);

                            let key = DelayCompKey {
                                delay: delay_comp_info.delay as u32,
                                src_node_ref: delay_comp_info.source_node._node_ref(),
                                port_stable_id: port_channel_id.port_stable_id,
                                port_channel_index: port_channel_id.port_channel,
                            };

                            let delay_comp_node = if let Some(delay_node) =
                                shared_pool.delay_comp_nodes.pool.get_mut(&key)
                            {
                                delay_node.active = true;
                                delay_node.clone()
                            } else {
                                let new_delay_node = SharedDelayCompNode::new(
                                    DelayCompNode::new(delay_comp_info.delay as u32),
                                    coll_handle,
                                );
                                let _ = shared_pool
                                    .delay_comp_nodes
                                    .pool
                                    .insert(key, new_delay_node.clone());
                                new_delay_node
                            };

                            tasks.push(Task::DelayComp(DelayCompTask {
                                shared_node: delay_comp_node,
                                audio_in: graph_buffer,
                                audio_out: intermediary_buffer.clone(),
                            }));

                            intermediary_buffer
                        } else {
                            graph_buffer
                        };

                        channel_buffers.push(channel_buffer);
                    }

                    if channel_buffers.is_empty() {
                        return Err(GraphCompilerError::UnexpectedError(format!(
                            "Abstract schedule gave no buffer for input channel {} on the plugin instance {:?}",
                            channel_index,
                            abstract_task.node,
                        ), abstract_tasks.to_vec()));
                    }

                    let channel_buffer = if channel_buffers.len() == 1 {
                        channel_buffers[0].clone()
                    } else {
                        // Add an intermediate buffer for the sum task.
                        let intermediary_buffer = shared_pool
                            .buffers
                            .intermediary_audio_buffer_pool
                            .initialized_buffer_at_index(intermediary_buffer_i);
                        intermediary_buffer_i += 1;
                        total_intermediary_buffers =
                            total_intermediary_buffers.max(intermediary_buffer_i);

                        tasks.push(Task::Sum(SumTask {
                            audio_in: channel_buffers,
                            audio_out: intermediary_buffer.clone(),
                        }));

                        intermediary_buffer
                    };

                    plugin_in_channel_buffers[channel_index] = Some(channel_buffer);
                }
                PortType::ParamAutomation => {
                    let mut bufs = SmallVec::with_capacity(buffers.len());

                    for (buffer, _delay_comp_info) in buffers.iter() {
                        // TODO: Use delay compensation?

                        max_graph_automation_buffer_index =
                            max_graph_automation_buffer_index.max(buffer.buffer_id);

                        let buffer = shared_pool
                            .buffers
                            .param_event_buffer_pool
                            .buffer_at_index(buffer.buffer_id);

                        bufs.push(buffer);
                    }

                    automation_in_buffers = Some(bufs);
                }
                PortType::Note => {
                    if let Some(note_ports_ext) = note_ports_ext {
                        // TODO: Optimize this?
                        for (port_i, port) in note_ports_ext.inputs.iter().enumerate() {
                            if port_channel_id.port_stable_id == port.stable_id {
                                let mut bufs = SmallVec::with_capacity(buffers.len());

                                for (buffer, _delay_comp_info) in buffers.iter() {
                                    // TODO: Use delay compensation?

                                    max_graph_note_buffer_index =
                                        max_graph_note_buffer_index.max(buffer.buffer_id);

                                    let buffer = shared_pool
                                        .buffers
                                        .note_buffer_pool
                                        .buffer_at_index(buffer.buffer_id);

                                    bufs.push(buffer);
                                }

                                note_in_buffers[port_i] = Some(bufs);

                                break;
                            }
                        }
                    }
                }
            }
        }

        for (port_channel_id, buffer) in abstract_task.outputs.iter() {
            match port_channel_id.port_type {
                PortType::Audio => {
                    max_graph_audio_buffer_index =
                        max_graph_audio_buffer_index.max(buffer.buffer_id);

                    let channel_index = if let Some(audio_ports_ext) = audio_ports_ext {
                        match audio_ports_ext.out_channel_index(
                            port_channel_id.port_stable_id,
                            port_channel_id.port_channel,
                        ) {
                            Some(index) => index,
                            None => {
                                return Err(GraphCompilerError::UnexpectedError(format!(
                                    "Abstract schedule refers to an output port with ID {} and channel {} in the plugin {:?} that doesn't exist",
                                    port_channel_id.port_stable_id,
                                    port_channel_id.port_channel,
                                    abstract_task.node,
                                ), abstract_tasks.to_vec()));
                            }
                        }
                    } else {
                        next_audio_out_channel_index += 1;
                        next_audio_out_channel_index - 1
                    };

                    let graph_buffer = shared_pool
                        .buffers
                        .audio_buffer_pool
                        .initialized_buffer_at_index(buffer.buffer_id);
                    plugin_out_channel_buffers[channel_index] = Some(graph_buffer);
                }
                PortType::ParamAutomation => {
                    max_graph_automation_buffer_index =
                        max_graph_automation_buffer_index.max(buffer.buffer_id);

                    let buffer = shared_pool
                        .buffers
                        .param_event_buffer_pool
                        .buffer_at_index(buffer.buffer_id);

                    automation_out_buffer = Some(buffer);
                }
                PortType::Note => {
                    if let Some(note_ports_ext) = note_ports_ext {
                        // TODO: Optimize this?
                        for (port_i, port) in note_ports_ext.outputs.iter().enumerate() {
                            if port_channel_id.port_stable_id == port.stable_id {
                                max_graph_note_buffer_index =
                                    max_graph_note_buffer_index.max(buffer.buffer_id);

                                let buffer = shared_pool
                                    .buffers
                                    .note_buffer_pool
                                    .buffer_at_index(buffer.buffer_id);

                                note_out_buffers[port_i] = Some(buffer);

                                break;
                            }
                        }
                    }
                }
            }
        }

        let mut audio_in_channel_buffers: SmallVec<[SharedBuffer<f32>; 4]> =
            SmallVec::with_capacity(num_audio_in_channels);
        let mut audio_out_channel_buffers: SmallVec<[SharedBuffer<f32>; 4]> =
            SmallVec::with_capacity(num_audio_out_channels);
        for i in 0..num_audio_in_channels {
            if let Some(buffer) = plugin_in_channel_buffers[i].take() {
                audio_in_channel_buffers.push(buffer);
            } else {
                return Err(GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule did not assign buffer for input audio channel at index {} for the plugin {:?}",
                    i,
                    abstract_task.node,
                ), abstract_tasks.to_vec()));
            }
        }
        for i in 0..num_audio_out_channels {
            if let Some(buffer) = plugin_out_channel_buffers[i].take() {
                audio_out_channel_buffers.push(buffer);
            } else {
                return Err(GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule did not assign buffer for output audio channel at index {} for the plugin {:?}",
                    i,
                    abstract_task.node,
                ), abstract_tasks.to_vec()));
            }
        }

        if &abstract_task.node == graph_in_node_id {
            graph_audio_in = audio_out_channel_buffers;
            continue;
        }
        if &abstract_task.node == graph_out_node_id {
            graph_audio_out = audio_in_channel_buffers;
            continue;
        }

        if shared_processor.is_none() {
            let mut audio_through: SmallVec<[(SharedBuffer<f32>, SharedBuffer<f32>); 4]> =
                SmallVec::new();
            let mut extra_audio_out: SmallVec<[SharedBuffer<f32>; 4]> = SmallVec::new();

            // Plugin is unloaded/deactivated.
            let mut port_i = 0;

            if let Some(audio_ports_ext) = audio_ports_ext {
                if let MainPortsLayout::InOut = audio_ports_ext.main_ports_layout {
                    let n_main_channels =
                        audio_ports_ext.inputs[0].channels.min(audio_ports_ext.outputs[0].channels);

                    for _ in 0..n_main_channels {
                        audio_through.push((
                            audio_in_channel_buffers[port_i].clone(),
                            audio_out_channel_buffers[port_i].clone(),
                        ));
                        port_i += 1;
                    }
                }
            }

            for i in port_i..audio_out_channel_buffers.len() {
                extra_audio_out.push(audio_out_channel_buffers[i].clone());
            }

            tasks.push(Task::DeactivatedPlugin(DeactivatedPluginTask {
                audio_through,
                extra_audio_out,
                automation_out_buffer,
                note_out_buffers,
            }));

            continue;
        }

        let shared_processor = shared_processor.unwrap();
        let audio_ports_ext = audio_ports_ext.as_ref().unwrap();

        let mut audio_in: SmallVec<[AudioPortBuffer; 2]> = SmallVec::new();
        let mut audio_out: SmallVec<[AudioPortBufferMut; 2]> = SmallVec::new();

        let mut port_i = 0;
        for in_port in audio_ports_ext.inputs.iter() {
            let mut buffers: SmallVec<[SharedBuffer<f32>; 2]> =
                SmallVec::with_capacity(usize::from(in_port.channels));
            for _ in 0..in_port.channels {
                buffers.push(audio_in_channel_buffers[port_i].clone());
                port_i += 1;
            }

            audio_in.push(AudioPortBuffer::_new(
                buffers,
                shared_pool.buffers.audio_buffer_pool.buffer_size() as u32,
            ));
            // TODO: proper latency?
        }
        port_i = 0;
        for out_port in audio_ports_ext.outputs.iter() {
            let mut buffers: SmallVec<[SharedBuffer<f32>; 2]> =
                SmallVec::with_capacity(usize::from(out_port.channels));
            for _ in 0..out_port.channels {
                buffers.push(audio_out_channel_buffers[port_i].clone());
                port_i += 1;
            }

            audio_out.push(AudioPortBufferMut::_new(
                buffers,
                shared_pool.buffers.audio_buffer_pool.buffer_size() as u32,
            ));
            // TODO: proper latency?
        }

        let shared_processor = shared_processor.clone();

        tasks.push(Task::Plugin(PluginTask {
            plugin_id: plugin_id.clone(),
            shared_processor,
            buffers: ProcBuffers { audio_in, audio_out },
            event_buffers: PluginEventIoBuffers {
                unmixed_param_in_buffers: automation_in_buffers,
                param_out_buffer: automation_out_buffer,
                unmixed_note_in_buffers: note_in_buffers,
                note_out_buffers,
            },
        }));
    }

    let new_schedule = Schedule::new(
        tasks,
        graph_audio_in,
        graph_audio_out,
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

#[derive(Debug)]
pub enum GraphCompilerError {
    VerifierError(
        VerifyScheduleError,
        Vec<ScheduledNode<PluginInstanceID, PortChannelID, PortType>>,
        Schedule,
    ),
    UnexpectedError(String, Vec<ScheduledNode<PluginInstanceID, PortChannelID, PortType>>),
}

impl Error for GraphCompilerError {}

impl std::fmt::Display for GraphCompilerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            GraphCompilerError::VerifierError(e, abstract_schedule, schedule) => {
                write!(f, "Failed to compile audio graph: {}\n\nOutput of abstract graph compiler: {:?}\n\nOutput of final compiler: {:?}", e, &abstract_schedule, &schedule)
            }
            GraphCompilerError::UnexpectedError(e, abstract_schedule) => {
                write!(f, "Failed to compile audio graph: Unexpected error: {}\n\nOutput of abstract graph compiler: {:?}", e, &abstract_schedule)
            }
        }
    }
}
