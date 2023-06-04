use basedrop::{Collector, Handle};

/// This struct collects data that is dropped in the realtime thread and
/// safely drops them at a later point in a separate thread.. This is
/// needed because deallocating memory on the heap is not a
/// realtime-safe operation.
///
/// This struct does not automatically collect garbage. You must
/// manually call `RtGarbageCollector::collect()` at regular intervals.
///
/// NOTE, this struct must not be dropped while there are any live
/// handles, or else the program will leak memory.
pub struct RtGarbageCollector {
    collector: Option<Collector>,
}

impl RtGarbageCollector {
    /// This method must be called at regular intervals.
    pub fn collect(&mut self) {
        if let Some(collector) = &mut self.collector {
            collector.collect();
        }
    }

    /// Create a new handle to this garbage collector.
    pub fn new_handle(&self) -> RtGCHandle {
        let h = self.collector.as_ref().unwrap().handle();

        RtGCHandle { h }
    }
}

impl Drop for RtGarbageCollector {
    fn drop(&mut self) {
        if let Some(mut collector) = self.collector.take() {
            collector.collect();

            if let Err(_) = collector.try_cleanup() {
                log::error!("GarbageCollector was dropped while handles exist!")
            }
        }
    }
}

/// A handle to the realtime garbage collector. This garbage collector
/// collects data that is dropped in the realtime thread and safely
/// drops them at a later point in a separate thread.
///
/// NOTE, all handles must be dropped before the main `GarbageCollector`
/// gets dropped, or else the program will leak memory.
pub struct RtGCHandle {
    h: Handle,
}

impl RtGCHandle {
    /// Retrieve the underlying `basedrop` handle.
    pub fn handle(&self) -> &Handle {
        &self.h
    }
}

impl Clone for RtGCHandle {
    fn clone(&self) -> Self {
        Self { h: self.h.clone() }
    }
}
