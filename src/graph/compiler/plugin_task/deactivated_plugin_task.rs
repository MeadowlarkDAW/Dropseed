use audio_graph::ScheduledNode;
use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ext::audio_ports::{MainPortsLayout, PluginAudioPortsExt};
use dropseed_plugin_api::ext::note_ports::PluginNotePortsExt;
use fnv::FnvHashMap;
use smallvec::SmallVec;

use crate::plugin_host::event_io_buffers::{NoteIoEvent, ParamIoEvent};
use crate::processor_schedule::tasks::{DeactivatedPluginTask, Task};

use super::super::super::error::GraphCompilerError;
use super::super::super::{ChannelID, PortType};

/// In this task, audio and note data is passed through the main ports (if the plugin
/// has main in/out ports), and then all the other output buffers are cleared.
pub(super) fn construct_deactivated_plugin_task(
    scheduled_node: &ScheduledNode,
    maybe_audio_ports_ext: Option<&PluginAudioPortsExt>,
    maybe_note_ports_ext: Option<&PluginNotePortsExt>,
    mut assigned_audio_buffers: FnvHashMap<ChannelID, (SharedBuffer<f32>, bool)>,
    mut assigned_note_buffers: FnvHashMap<ChannelID, (SharedBuffer<NoteIoEvent>, bool)>,
    assigned_param_event_out_buffer: Option<SharedBuffer<ParamIoEvent>>,
) -> Result<Task, GraphCompilerError> {
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
