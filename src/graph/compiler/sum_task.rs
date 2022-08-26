use audio_graph::InsertedSum;
use dropseed_plugin_api::buffer::SharedBuffer;
use smallvec::SmallVec;

use crate::plugin_host::event_io_buffers::{NoteIoEvent, ParamIoEvent};
use crate::processor_schedule::tasks::{AudioSumTask, NoteSumTask, ParamEventSumTask, Task};

use super::super::error::GraphCompilerError;
use super::super::shared_pools::GraphSharedPools;
use super::super::PortType;

pub(super) fn construct_sum_task(
    inserted_sum: &InsertedSum,
    shared_pool: &mut GraphSharedPools,
) -> Result<Task, GraphCompilerError> {
    let task = match inserted_sum.output_buffer.type_index {
        PortType::AUDIO_TYPE_IDX => {
            let audio_in: SmallVec<[SharedBuffer<f32>; 4]> = inserted_sum
                .input_buffers
                .iter()
                .map(|assigned_buffer| {
                    shared_pool
                        .buffers
                        .audio_buffer_pool
                        .initialized_buffer_at_index(assigned_buffer.buffer_index.0)
                })
                .collect();
            let audio_out = shared_pool
                .buffers
                .audio_buffer_pool
                .initialized_buffer_at_index(inserted_sum.output_buffer.buffer_index.0);

            Task::AudioSum(AudioSumTask { audio_in, audio_out })
        }
        PortType::NOTE_TYPE_IDX => {
            let note_in: SmallVec<[SharedBuffer<NoteIoEvent>; 4]> = inserted_sum
                .input_buffers
                .iter()
                .map(|assigned_buffer| {
                    shared_pool
                        .buffers
                        .note_buffer_pool
                        .buffer_at_index(assigned_buffer.buffer_index.0)
                })
                .collect();
            let note_out = shared_pool
                .buffers
                .note_buffer_pool
                .buffer_at_index(inserted_sum.output_buffer.buffer_index.0);

            Task::NoteSum(NoteSumTask { note_in, note_out })
        }
        PortType::PARAM_AUTOMATION_TYPE_IDX => {
            let event_in: SmallVec<[SharedBuffer<ParamIoEvent>; 4]> = inserted_sum
                .input_buffers
                .iter()
                .map(|assigned_buffer| {
                    shared_pool
                        .buffers
                        .param_event_buffer_pool
                        .buffer_at_index(assigned_buffer.buffer_index.0)
                })
                .collect();
            let event_out = shared_pool
                .buffers
                .param_event_buffer_pool
                .buffer_at_index(inserted_sum.output_buffer.buffer_index.0);

            Task::ParamEventSum(ParamEventSumTask { event_in, event_out })
        }
        _ => {
            return Err(GraphCompilerError::UnexpectedError(format!(
                "Abstract schedule inserted a sum with unkown type index {:?}",
                inserted_sum
            )));
        }
    };

    Ok(task)
}
