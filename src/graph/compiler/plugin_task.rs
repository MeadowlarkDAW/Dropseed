use audio_graph::ScheduledNode;
use dropseed_plugin_api::buffer::SharedBuffer;
use fnv::FnvHashMap;

use crate::plugin_host::event_io_buffers::{NoteIoEvent, ParamIoEvent};
use crate::processor_schedule::tasks::Task;

use super::super::error::GraphCompilerError;
use super::super::shared_pools::GraphSharedPools;
use super::super::{ChannelID, PortType};

mod activated_plugin_task;
mod deactivated_plugin_task;

pub(super) fn construct_plugin_task(
    scheduled_node: &ScheduledNode,
    shared_pool: &mut GraphSharedPools,
) -> Result<Task, GraphCompilerError> {
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
        deactivated_plugin_task::construct_deactivated_plugin_task(
            scheduled_node,
            maybe_audio_ports_ext,
            maybe_note_ports_ext,
            assigned_audio_buffers,
            assigned_note_buffers,
            assigned_param_event_out_buffer,
        )
    } else {
        activated_plugin_task::construct_activated_plugin_task(
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
