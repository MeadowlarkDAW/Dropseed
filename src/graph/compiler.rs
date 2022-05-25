use audio_graph::{Graph, ScheduledNode};
use basedrop::Shared;
use smallvec::SmallVec;
use std::error::Error;

use crate::graph::shared_pool::{DelayCompKey, SharedDelayCompNode};
use crate::plugin::ext::audio_ports::MainPortsLayout;
use crate::plugin::process_info::ProcBuffers;
use crate::{AudioPortBuffer, AudioPortBufferMut, HostInfo};

use super::{
    schedule::sum::SumTask,
    schedule::task::{DeactivatedPluginTask, DelayCompTask, PluginTask, Task},
    shared_pool::{SharedBuffer, SharedPool},
    verifier::{Verifier, VerifyScheduleError},
    DefaultPortType, PluginInstanceID, PortID, Schedule,
};

pub(crate) fn compile_graph(
    shared_pool: &mut SharedPool,
    abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
    graph_in_node_id: &PluginInstanceID,
    graph_out_node_id: &PluginInstanceID,
    verifier: &mut Verifier,
    host_info: &Shared<HostInfo>,
    coll_handle: &basedrop::Handle,
) -> Result<Schedule, GraphCompilerError> {
    let num_plugins = shared_pool.plugins.len();

    let mut tasks: Vec<Task> = Vec::with_capacity(num_plugins * 2);

    let mut graph_audio_in: SmallVec<[SharedBuffer<f32>; 4]> = SmallVec::new();
    let mut graph_audio_out: SmallVec<[SharedBuffer<f32>; 4]> = SmallVec::new();

    let mut total_intermediary_buffers = 0;
    let mut max_graph_audio_buffer_index = 0;

    for node in shared_pool.delay_comp_nodes.values_mut() {
        node.active = false;
    }

    let abstract_tasks = abstract_graph.compile();
    for abstract_task in abstract_tasks.iter() {
        let (num_input_channels, num_output_channels, plugin_audio_thread, audio_ports_ext) =
            if let Some(entry) = shared_pool.plugins.get(&abstract_task.node) {
                (
                    entry.audio_in_channel_refs.len(),
                    entry.audio_out_channel_refs.len(),
                    entry.audio_thread.as_ref().cloned(),
                    &entry.plugin_host.audio_ports_ext,
                )
            } else {
                return Err(GraphCompilerError::UnexpectedError(format!(
                "Abstract schedule refers to a plugin instance {:?} that does not exist in the plugin pool",
                abstract_task.node
            ), abstract_tasks.to_vec()));
            };

        let mut intermediary_buffer_i = 0;

        let mut plugin_in_channel_buffers: SmallVec<[Option<SharedBuffer<f32>>; 4]> =
            SmallVec::with_capacity(num_input_channels);
        let mut plugin_out_channel_buffers: SmallVec<[Option<SharedBuffer<f32>>; 4]> =
            SmallVec::with_capacity(num_output_channels);
        for _ in 0..num_input_channels {
            plugin_in_channel_buffers.push(None);
        }
        for _ in 0..num_output_channels {
            plugin_out_channel_buffers.push(None);
        }

        for (channel, buffers) in abstract_task.inputs.iter() {
            let channel_index = channel.as_index();
            if channel_index >= num_input_channels {
                return Err(GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule refers to an input channel at index {} in the plugin {:?} which only has {} input channels",
                    channel_index,
                    abstract_task.node,
                    num_input_channels
                ), abstract_tasks.to_vec()));
            }

            let mut channel_buffers: SmallVec<[SharedBuffer<f32>; 4]> = SmallVec::new();
            for (buffer, delay_comp_info) in buffers.iter() {
                max_graph_audio_buffer_index = max_graph_audio_buffer_index.max(buffer.buffer_id);

                let graph_buffer = shared_pool.audio_f32(buffer.buffer_id);

                let channel_buffer = if let Some(delay_comp_info) = &delay_comp_info {
                    // Add an intermediate buffer for the delay compensation task.
                    let intermediary_buffer =
                        shared_pool.intermediary_audio_f32(intermediary_buffer_i);

                    intermediary_buffer_i += 1;
                    total_intermediary_buffers =
                        total_intermediary_buffers.max(intermediary_buffer_i);

                    let key = DelayCompKey {
                        delay: delay_comp_info.delay as u32,
                        src_node_ref: delay_comp_info.source_node.node_ref,
                        port_i: delay_comp_info.source_port.as_index() as u16,
                    };

                    let delay_comp_node = if let Some(delay_node) =
                        shared_pool.delay_comp_nodes.get_mut(&key)
                    {
                        delay_node.active = true;
                        delay_node.clone()
                    } else {
                        let new_delay_node =
                            SharedDelayCompNode::new(delay_comp_info.delay as u32, coll_handle);
                        let _ = shared_pool.delay_comp_nodes.insert(key, new_delay_node.clone());
                        new_delay_node
                    };

                    tasks.push(Task::DelayComp(DelayCompTask {
                        delay_comp_node,
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
                let intermediary_buffer = shared_pool.intermediary_audio_f32(intermediary_buffer_i);
                intermediary_buffer_i += 1;
                total_intermediary_buffers = total_intermediary_buffers.max(intermediary_buffer_i);

                tasks.push(Task::Sum(SumTask {
                    audio_in: channel_buffers,
                    audio_out: intermediary_buffer.clone(),
                }));

                intermediary_buffer
            };

            plugin_in_channel_buffers[channel_index] = Some(channel_buffer);
        }

        for (channel, buffer) in abstract_task.outputs.iter() {
            max_graph_audio_buffer_index = max_graph_audio_buffer_index.max(buffer.buffer_id);

            let channel_index = channel.as_index();
            if channel_index >= num_output_channels {
                return Err(GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule refers to an output channel at index {} in the plugin {:?} which only has {} output channels",
                    channel_index,
                    abstract_task.node,
                    num_output_channels
                ), abstract_tasks.to_vec()));
            }

            let graph_buffer = shared_pool.audio_f32(buffer.buffer_id);
            plugin_out_channel_buffers[channel_index] = Some(graph_buffer);
        }

        let mut audio_in_channel_buffers: SmallVec<[SharedBuffer<f32>; 4]> =
            SmallVec::with_capacity(num_input_channels);
        let mut audio_out_channel_buffers: SmallVec<[SharedBuffer<f32>; 4]> =
            SmallVec::with_capacity(num_output_channels);
        for i in 0..num_input_channels {
            if let Some(buffer) = plugin_in_channel_buffers[i].take() {
                audio_in_channel_buffers.push(buffer);
            } else {
                return Err(GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule did not assign buffer for input channel at index {} for the plugin {:?}",
                    i,
                    abstract_task.node,
                ), abstract_tasks.to_vec()));
            }
        }
        for i in 0..num_output_channels {
            if let Some(buffer) = plugin_out_channel_buffers[i].take() {
                audio_out_channel_buffers.push(buffer);
            } else {
                return Err(GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule did not assign buffer for output channel at index {} for the plugin {:?}",
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

        if plugin_audio_thread.is_none() {
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
            }));

            continue;
        }

        let plugin_audio_thread = plugin_audio_thread.unwrap();
        let audio_ports_ext = audio_ports_ext.unwrap();

        let mut audio_in: SmallVec<[AudioPortBuffer; 2]> = SmallVec::new();
        let mut audio_out: SmallVec<[AudioPortBufferMut; 2]> = SmallVec::new();

        let mut port_i = 0;
        for in_port in audio_ports_ext.inputs.iter() {
            let mut buffers: SmallVec<[SharedBuffer<f32>; 2]> =
                SmallVec::with_capacity(in_port.channels);
            for _ in 0..in_port.channels {
                buffers.push(audio_in_channel_buffers[port_i].clone());
                port_i += 1;
            }

            audio_in.push(AudioPortBuffer::new(buffers, shared_pool.buffer_size as u32));
            // TODO: proper latency?
        }
        port_i = 0;
        for out_port in audio_ports_ext.outputs.iter() {
            let mut buffers: SmallVec<[SharedBuffer<f32>; 2]> =
                SmallVec::with_capacity(out_port.channels);
            for _ in 0..out_port.channels {
                buffers.push(audio_out_channel_buffers[port_i].clone());
                port_i += 1;
            }

            audio_out.push(AudioPortBufferMut::new(buffers, shared_pool.buffer_size as u32));
            // TODO: proper latency?
        }

        let task_version = plugin_audio_thread.task_version;

        tasks.push(Task::Plugin(PluginTask {
            plugin: plugin_audio_thread,
            buffers: ProcBuffers { audio_in, audio_out, task_version },
        }));
    }

    let new_schedule = Schedule {
        tasks,
        graph_audio_in,
        graph_audio_out,
        max_block_size: shared_pool.buffer_size,
        host_info: Shared::clone(&host_info),
    };

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

    shared_pool
        .remove_excess_audio_buffers(max_graph_audio_buffer_index, total_intermediary_buffers);

    // TODO: Optimize this?
    //
    // Not stable yet apparently
    // plugin_pool.delay_comp_nodes.drain_filter(|_, node| !node.active);
    shared_pool.delay_comp_nodes =
        shared_pool.delay_comp_nodes.drain().filter(|(_, node)| node.active).collect();

    Ok(new_schedule)
}

#[derive(Debug)]
pub enum GraphCompilerError {
    VerifierError(
        VerifyScheduleError,
        Vec<ScheduledNode<PluginInstanceID, PortID, DefaultPortType>>,
        Schedule,
    ),
    UnexpectedError(String, Vec<ScheduledNode<PluginInstanceID, PortID, DefaultPortType>>),
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
