use audio_graph::Graph;
use smallvec::SmallVec;
use std::error::Error;

use crate::graph::plugin_pool::{DelayCompKey, SharedDelayCompNode};
use crate::plugin_scanner::PluginFormat;
use crate::AudioPortBuffer;

use super::{
    audio_buffer_pool::{AudioBufferPool, SharedAudioBuffer},
    plugin_pool::PluginInstancePool,
    schedule::task::{DelayCompTask, InactivePluginTask, InternalPluginTask, SumTask, Task},
    DefaultPortType, PluginInstanceID, PortID, Schedule,
};

pub(crate) fn compile_graph(
    plugin_pool: &mut PluginInstancePool,
    audio_buffer_pool: &mut AudioBufferPool,
    abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
    graph_in_node_id: &PluginInstanceID,
    graph_out_node_id: &PluginInstanceID,
) -> Result<Schedule, GraphCompilerError> {
    let mut tasks: Vec<Task> = Vec::with_capacity(plugin_pool.num_plugins() * 2);

    let mut total_intermidiary_buffers = 0;

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
            )));
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
                )));
            }

            let mut channel_buffers: SmallVec<[SharedAudioBuffer<f32>; 4]> = SmallVec::new();
            for (buffer, delay_comp_info) in buffers.iter() {
                let graph_buffer = audio_buffer_pool.get_graph_buffer(buffer.buffer_id);

                let channel_buffer = if let Some(delay_comp_info) = &delay_comp_info {
                    // Add an intermediate buffer for the delay compensation task.
                    let intermediary_buffer =
                        audio_buffer_pool.get_intermediary_buffer(intermediary_buffer_i);
                    intermediary_buffer_i += 1;
                    total_intermidiary_buffers =
                        total_intermidiary_buffers.max(intermediary_buffer_i);

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
                )));
            }

            let channel_buffer = if channel_buffers.len() == 1 {
                channel_buffers[0].clone()
            } else {
                // Add an intermediate buffer for the sum task.
                let intermediary_buffer =
                    audio_buffer_pool.get_intermediary_buffer(intermediary_buffer_i);
                intermediary_buffer_i += 1;
                total_intermidiary_buffers = total_intermidiary_buffers.max(intermediary_buffer_i);

                tasks.push(Task::Sum(SumTask {
                    audio_in: channel_buffers,
                    audio_out: intermediary_buffer.clone(),
                }));

                intermediary_buffer
            };

            plugin_in_channel_buffers[channel_index] = Some(channel_buffer);
        }

        for (channel, buffer) in abstract_task.outputs.iter() {
            let channel_index = channel.as_index();
            if channel_index >= num_output_channels {
                return Err(GraphCompilerError::UnexpectedError(format!(
                    "Abstract schedule refers to an output channel at index {} in the plugin {:?} which only has {} output channels",
                    channel_index,
                    abstract_task.node,
                    num_output_channels
                )));
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
                )));
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
                )));
            }
        }

        if &abstract_task.node == graph_in_node_id {}

        if plugin_pool.is_plugin_active(&abstract_task.node).unwrap() {
            tasks.push(Task::InactivePlugin(InactivePluginTask {
                audio_out: audio_out_channel_buffers,
            }));

            continue;
        }

        match plugin_format {
            PluginFormat::Internal => {
                let mut audio_in: SmallVec<[AudioPortBuffer; 2]> = SmallVec::new();
                let mut audio_out: SmallVec<[AudioPortBuffer; 2]> = SmallVec::new();

                // Unwrap won't panic because it can only be `None` when the plugin is inactive,
                // and we already checked for that above.
                let audio_ports_ext =
                    plugin_pool.get_audio_ports_ext(&abstract_task.node).unwrap().as_ref().unwrap();

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

                // Unwrap won't panic because this is only `None` when the plugin is inactive,
                // and we already checked for that above.
                let plugin_audio_thread = plugin_pool
                    .get_graph_plugin_audio_thread(&abstract_task.node)
                    .unwrap()
                    .unwrap();

                tasks.push(Task::InternalPlugin(InternalPluginTask {
                    plugin: plugin_audio_thread.clone(),
                    audio_in,
                    audio_out,
                }));
            }
            PluginFormat::Clap => {
                todo!()
            }
        }
    }

    todo!()
}

#[derive(Debug)]
pub enum GraphCompilerError {
    UnexpectedError(String),
}

impl Error for GraphCompilerError {}

impl std::fmt::Display for GraphCompilerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            GraphCompilerError::UnexpectedError(e) => {
                write!(f, "Failed to compile audio graph: Unexpected error: {}", e)
            }
        }
    }
}
