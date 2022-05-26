use basedrop::{Shared, SharedCell};
use std::thread::ThreadId;

pub(crate) struct SharedThreadIDs {
    // TODO: Use AtomicU64 instead once ThreadId::as_u64() becomes stable?
    external_main_thread_id: Shared<SharedCell<Option<ThreadId>>>,
    external_audio_thread_id: Shared<SharedCell<Option<ThreadId>>>,
}

impl Clone for SharedThreadIDs {
    fn clone(&self) -> Self {
        Self {
            external_main_thread_id: Shared::clone(&self.external_main_thread_id),
            external_audio_thread_id: Shared::clone(&self.external_audio_thread_id),
        }
    }
}

impl SharedThreadIDs {
    pub fn new(
        external_main_thread_id: Option<ThreadId>,
        external_audio_thread_id: Option<ThreadId>,
        coll_handle: &basedrop::Handle,
    ) -> Self {
        Self {
            external_main_thread_id: Shared::new(
                coll_handle,
                SharedCell::new(Shared::new(coll_handle, external_main_thread_id)),
            ),
            external_audio_thread_id: Shared::new(
                coll_handle,
                SharedCell::new(Shared::new(coll_handle, external_audio_thread_id)),
            ),
        }
    }

    pub fn external_main_thread_id(&self) -> Option<ThreadId> {
        *self.external_main_thread_id.get()
    }

    pub fn external_audio_thread_id(&self) -> Option<ThreadId> {
        *self.external_audio_thread_id.get()
    }

    pub fn set_external_main_thread_id(&self, id: ThreadId, coll_handle: &basedrop::Handle) {
        self.external_main_thread_id.set(Shared::new(coll_handle, Some(id)));
    }

    pub fn set_external_audio_thread_id(&self, id: ThreadId, coll_handle: &basedrop::Handle) {
        self.external_audio_thread_id.set(Shared::new(coll_handle, Some(id)));
    }
}
