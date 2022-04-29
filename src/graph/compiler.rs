use audio_graph::{Graph, ScheduledNode};
use basedrop::Shared;
use smallvec::SmallVec;
use std::error::Error;

use crate::graph::plugin_pool::{DelayCompKey, SharedDelayCompNode};
use crate::plugin::ext::audio_ports::MainPortsLayout;
use crate::plugin_scanner::PluginFormat;
use crate::AudioPortBuffer;

use super::schedule;
use super::{
    audio_buffer_pool::{AudioBufferPool, SharedAudioBuffer},
    plugin_pool::PluginInstancePool,
    schedule::task::{DeactivatedPluginTask, DelayCompTask, InternalPluginTask, SumTask, Task},
    verifier::{Verifier, VerifyScheduleError},
    DefaultPortType, PluginInstanceID, PortID, Schedule,
};

pub(crate) fn compile_graph(
    plugin_pool: &mut PluginInstancePool,
    audio_buffer_pool: &mut AudioBufferPool,
    abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
    graph_in_node_id: &PluginInstanceID,
    graph_out_node_id: &PluginInstanceID,
    verifier: &mut Verifier,
) -> Result<Schedule, GraphCompilerError> {
    let mut tasks: Vec<Task> = Vec::with_capacity(plugin_pool.num_plugins() * 2);

    let mut graph_audio_in: SmallVec<[SharedAudioBuffer<f32>; 4]> = SmallVec::new();
    let mut graph_audio_out: SmallVec<[SharedAudioBuffer<f32>; 4]> = SmallVec::new();

    let mut total_intermediary_buffers = 0;
    let mut max_graph_audio_buffer_id = 0;

    for node in plugin_pool.delay_comp_nodes.values_mut() {
        node.active = false;
    }

    let abstract_tasks = abstract_graph.compile();
    for abstract_task in abstract_tasks.iter() {
        let num_input_channels = if let Ok(c) =
            plugin_pool.get_audio_in_channel_refs(&abstract_task.node)
        {
            c.len()
        } else {
            return Err(GraphCompilerError::UnexpectedError(format!(
                "Abstract schedule refers to a plugin instance {:?} that does not exist in the plugin pool",
                abstract_task.node
            ), abstract_tasks.to_vec()));
        };
        let num_output_channels =
            plugin_pool.get_audio_out_channel_refs(&abstract_task.node).unwrap().len();
        let plugin_format = plugin_pool.get_plugin_format(&abstract_task.node).unwrap();

        let mut intermediary_buffer_i = 0;

        let mut plugin_in_channel_buffers: SmallVec<[Option<SharedAudioBuffer<f32>>; 4]> =
            SmallVec::with_capacity(num_input_channels);
        let mut plugin_out_channel_buffers: SmallVec<[Option<SharedAudioBuffer<f32>>; 4]> =
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

            let mut channel_buffers: SmallVec<[SharedAudioBuffer<f32>; 4]> = SmallVec::new();
            for (buffer, delay_comp_info) in buffers.iter() {
                max_graph_audio_buffer_id = max_graph_audio_buffer_id.max(buffer.buffer_id);

                let graph_buffer = audio_buffer_pool.get_graph_buffer(buffer.buffer_id);

                let channel_buffer = if let Some(delay_comp_info) = &delay_comp_info {
                    // Add an intermediate buffer for the delay compensation task.
                    let intermediary_buffer =
                        audio_buffer_pool.get_intermediary_buffer(intermediary_buffer_i);
                    intermediary_buffer_i += 1;
                    total_intermediary_buffers =
                        total_intermediary_buffers.max(intermediary_buffer_i);

                    let key = DelayCompKey {
                        delay: delay_comp_info.delay as u32,
                        node_id: delay_comp_info.source_node.node_id,
                        port_i: delay_comp_info.source_port.as_index() as u16,
                    };

                    let delay_comp_node = if let Some(delay_node) =
                        plugin_pool.delay_comp_nodes.get_mut(&key)
                    {
                        delay_node.active = true;
                        delay_node.clone()
                    } else {
                        let new_delay_node = SharedDelayCompNode::new(
                            delay_comp_info.delay as u32,
                            &audio_buffer_pool.coll_handle,
                        );
                        let _ = plugin_pool.delay_comp_nodes.insert(key, new_delay_node.clone());
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
                let intermediary_buffer =
                    audio_buffer_pool.get_intermediary_buffer(intermediary_buffer_i);
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
            max_graph_audio_buffer_id = max_graph_audio_buffer_id.max(buffer.buffer_id);

            let channel_index = channel.as_index();
            if channel_index >= num_output_channels {
                return Err(GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule refers to an output channel at index {} in the plugin {:?} which only has {} output channels",
                    channel_index,
                    abstract_task.node,
                    num_output_channels
                ), abstract_tasks.to_vec()));
            }

            let graph_buffer = audio_buffer_pool.get_graph_buffer(buffer.buffer_id);
            plugin_out_channel_buffers[channel_index] = Some(graph_buffer);
        }

        let mut audio_in_channel_buffers: SmallVec<[SharedAudioBuffer<f32>; 4]> =
            SmallVec::with_capacity(num_input_channels);
        let mut audio_out_channel_buffers: SmallVec<[SharedAudioBuffer<f32>; 4]> =
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

        match plugin_format {
            PluginFormat::Internal => {
                if let Some(plugin_audio_thread) =
                    plugin_pool.get_graph_plugin_audio_thread(&abstract_task.node).unwrap()
                {
                    let mut audio_in: SmallVec<[AudioPortBuffer; 2]> = SmallVec::new();
                    let mut audio_out: SmallVec<[AudioPortBuffer; 2]> = SmallVec::new();

                    // Won't panic because this is always `Some` when `get_graph_plugin_audio_thread()` is `Some`.
                    let audio_ports_ext =
                        plugin_pool.get_audio_ports_ext(&abstract_task.node).unwrap().unwrap();

                    let mut port_i = 0;
                    for in_port in audio_ports_ext.inputs.iter() {
                        let mut buffers: SmallVec<[SharedAudioBuffer<f32>; 2]> =
                            SmallVec::with_capacity(in_port.channels);
                        for _ in 0..in_port.channels {
                            buffers.push(audio_in_channel_buffers[port_i].clone());
                            port_i += 1;
                        }

                        audio_in.push(AudioPortBuffer::new(buffers, 0)); // TODO: latency?
                    }
                    port_i = 0;
                    for out_port in audio_ports_ext.outputs.iter() {
                        let mut buffers: SmallVec<[SharedAudioBuffer<f32>; 2]> =
                            SmallVec::with_capacity(out_port.channels);
                        for _ in 0..out_port.channels {
                            buffers.push(audio_out_channel_buffers[port_i].clone());
                            port_i += 1;
                        }

                        audio_out.push(AudioPortBuffer::new(buffers, 0)); // TODO: latency?
                    }

                    tasks.push(Task::InternalPlugin(InternalPluginTask {
                        plugin: plugin_audio_thread.clone(),
                        audio_in,
                        audio_out,
                    }));
                } else {
                    let mut audio_through: SmallVec<
                        [(SharedAudioBuffer<f32>, SharedAudioBuffer<f32>); 4],
                    > = SmallVec::new();
                    let mut extra_audio_out: SmallVec<[SharedAudioBuffer<f32>; 4]> =
                        SmallVec::new();

                    // Plugin is unloaded/deactivated.
                    let mut port_i = 0;
                    if let Some(audio_ports_ext) =
                        plugin_pool.get_audio_ports_ext(&abstract_task.node).unwrap()
                    {
                        if let MainPortsLayout::InOut = audio_ports_ext.main_ports_layout {
                            let n_main_channels = audio_ports_ext.inputs[0]
                                .channels
                                .min(audio_ports_ext.outputs[0].channels);

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
                }
            }
            PluginFormat::Clap => {
                todo!()
            }
        }
    }

    let new_schedule = Schedule {
        tasks,
        graph_audio_in,
        graph_audio_out,
        max_block_size: audio_buffer_pool.max_block_size(),
        host_info: Shared::clone(plugin_pool.host_info()),
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

    // Remove no longer needed buffers.
    //
    // The extra sanity error check is probably not necessary. I'll probably remove it once I'm sure
    // that it's sound.
    if let Err(num_buffers) =
        audio_buffer_pool.remove_unneeded_graph_buffers(max_graph_audio_buffer_id)
    {
        return Err(GraphCompilerError::UnexpectedError(format!(
            "The max graph audio buffer ID {} is not less than the total number of buffers that exist {}",
            max_graph_audio_buffer_id,
            num_buffers,
        ), abstract_tasks.to_vec()));
    }
    if let Err(num_buffers) =
        audio_buffer_pool.remove_unneeded_intermediary_buffers(total_intermediary_buffers)
    {
        return Err(GraphCompilerError::UnexpectedError(format!(
            "The number of intermediary buffers {} is greater than the total number of buffers that exist {}",
            total_intermediary_buffers,
            num_buffers,
        ), abstract_tasks.to_vec()));
    }

    // This could potentially be expensive, so maybe it should be done in the garbage
    // collector thread?
    //
    // Not stable yet apparently
    // plugin_pool.delay_comp_nodes.drain_filter(|_, node| !node.active);
    plugin_pool.delay_comp_nodes =
        plugin_pool.delay_comp_nodes.drain().filter(|(_, node)| node.active).collect();

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
