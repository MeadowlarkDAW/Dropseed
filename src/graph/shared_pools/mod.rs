mod buffer_pool;
mod delay_comp_node_pool;
mod plugin_host_pool;
mod shared_schedule;
mod transport_pool;

pub(crate) use buffer_pool::SharedBufferPool;
pub(crate) use delay_comp_node_pool::{DelayCompKey, DelayCompNodePool, SharedDelayCompNode};
pub(crate) use plugin_host_pool::PluginHostPool;
pub(crate) use shared_schedule::SharedSchedule;
pub(crate) use transport_pool::{SharedTransportTask, TransportPool};

use crate::{
    schedule::{tasks::TransportTask, Schedule},
    utils::thread_id::SharedThreadIDs,
};

pub(super) struct GraphSharedPools {
    pub shared_schedule: SharedSchedule,

    pub buffers: SharedBufferPool,
    pub plugin_hosts: PluginHostPool,
    pub delay_comp_nodes: DelayCompNodePool,
    pub transports: TransportPool,
}

impl GraphSharedPools {
    pub fn new(
        thread_ids: SharedThreadIDs,
        audio_buffer_size: usize,
        note_buffer_size: usize,
        event_buffer_size: usize,
        transport: TransportTask,
        coll_handle: basedrop::Handle,
    ) -> (Self, SharedSchedule) {
        let shared_transport_task = SharedTransportTask::new(transport, &coll_handle);

        let empty_schedule = Schedule::new_empty(audio_buffer_size, shared_transport_task.clone());

        let (shared_schedule, shared_schedule_clone) =
            SharedSchedule::new(empty_schedule, thread_ids, &coll_handle);

        (
            Self {
                shared_schedule,
                buffers: SharedBufferPool::new(
                    audio_buffer_size,
                    note_buffer_size,
                    event_buffer_size,
                    coll_handle,
                ),
                plugin_hosts: PluginHostPool::new(),
                delay_comp_nodes: DelayCompNodePool::new(),
                transports: TransportPool { transport: shared_transport_task },
            },
            shared_schedule_clone,
        )
    }
}
