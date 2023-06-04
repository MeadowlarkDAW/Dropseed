use bitflags::bitflags;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use std::hash::Hash;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimerID(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GuiSize {
    pub width: u32,
    pub height: u32,
}

impl GuiSize {
    pub(crate) fn to_u64(&self) -> u64 {
        ((self.width as u64) << 32) + self.height as u64
    }

    pub(crate) fn from_u64(v: u64) -> Self {
        Self { width: (v >> 32) as u32, height: (v & 0x00000000FFFFFFFF) as u32 }
    }
}

bitflags! {
    /// A bitmask of requests from a node.
    ///
    /// The host is free to not fulfill the request at its own discretion.
    pub struct NodeRequestFlags: u64 {
        /// Request to restart processing
        const RESTART = 1 << 0;

        /// Request to activate the node and start processing
        const PROCESS = 1 << 1;

        /// Request to call the on_main() callback
        const CALLBACK = 1 << 2;

        /// Request to rescan audio ports
        const RESCAN_AUDIO_PORTS = 1 << 3;

        /// Request to rescan audio port configurations
        const RESCAN_AUDIO_PORT_CONFIGS = 1 << 4;

        #[cfg(feature = "note-data")]
        /// Request to rescan note ports
        const RESCAN_NOTE_PORTS = 1 << 5;

        /// Request to rescan parameters
        const RESCAN_PARAMS = 1 << 6;

        /// Request to flush parameter values
        const FLUSH_PARAMS = 1 << 7;

        #[cfg(feature = "external-plugin-guis")]
        /// Request to update GUI resize hints
        const GUI_HINTS_CHANGED = 1 << 8;

        #[cfg(feature = "external-plugin-guis")]
        /// Request to show the GUI
        const GUI_SHOW = 1 << 9;

        #[cfg(feature = "external-plugin-guis")]
        /// Request to hide the GUI
        const GUI_HIDE = 1 << 10;

        #[cfg(feature = "external-plugin-guis")]
        /// Request to register the user closed the floating UI
        const GUI_CLOSED = 1 << 11;

        #[cfg(feature = "external-plugin-guis")]
        /// Request to register the connection to the UI was lost
        const GUI_DESTROYED = 1 << 12;

        /// Tell the host that the node's latency has changed.
        ///
        /// If the node is activated, then the host will deactivate and
        /// reactivate it in order to change the latency.
        const LATENCY_CHANGED = 1 << 13;

        /// The node has changed its state and it should be saved again.
        ///
        /// (Note that when a parameter value changes, it is implicit that
        /// the state is dirty and there is no need to set this flag.)
        const MARK_DIRTY = 1 << 13;

        #[cfg(feature = "note-data")]
        /// Request the host to rescan the plugin's note names.
        const RESCAN_NOTE_NAMES = 1 << 14;
    }
}

pub(crate) fn node_request_channel(
    #[cfg(feature = "external-plugin-guis")] node_supports_gui: bool,
    node_supports_timers: bool,
) -> (NodeRequestReceiver, NodeRequestMainThr, NodeRequestAudioThr) {
    let channel = Arc::new(NodeRequestChannel::new(
        #[cfg(feature = "external-plugin-guis")]
        node_supports_gui,
        node_supports_timers,
    ));

    (
        NodeRequestReceiver { channel: Arc::clone(&channel) },
        NodeRequestMainThr {
            channel: Arc::clone(&channel),
            next_timer_id: Arc::new(AtomicU32::new(0)),
        },
        NodeRequestAudioThr { channel },
    )
}

pub(crate) struct NodeRequestReceiver {
    channel: Arc<NodeRequestChannel>,
}

impl NodeRequestReceiver {
    /// Returns all the requests that have been made to the channel since the last call to [`fetch_requests`].
    ///
    /// This operation never blocks.
    #[inline]
    pub fn fetch_requests(&self) -> NodeRequestFlags {
        NodeRequestFlags::from_bits_retain(
            self.channel.request_flags.swap(NodeRequestFlags::empty().bits(), Ordering::SeqCst),
        )
    }

    #[cfg(feature = "external-plugin-guis")]
    /// Returns the last GUI size that was requested (through a call to [`request_gui_resize`](NodeRequestMainThr::request_gui_resize)).
    ///
    /// This returns [`None`] if no new size has been requested for this node yet.
    #[inline]
    pub fn fetch_requested_gui_size(&self) -> Option<GuiSize> {
        if let Some(requested_gui_size) = &self.channel.requested_gui_size {
            let maybe_size = requested_gui_size.swap(u64::MAX, Ordering::SeqCst);

            if maybe_size == u64::MAX {
                None
            } else {
                Some(GuiSize::from_u64(maybe_size))
            }
        } else {
            None
        }
    }

    #[inline]
    pub fn fetch_timer_requests(&mut self) -> Option<Vec<HostTimerRequest>> {
        if let Some(timer_requests) = &self.channel.timer_requests {
            let mut v = Vec::new();

            // Using a mutex here is okay because the node can't make timer requests
            // from the realtime thread.
            let mut r = timer_requests.lock().unwrap();
            if !r.is_empty() {
                std::mem::swap(&mut *r, &mut v)
            }

            Some(v)
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct NodeRequestMainThr {
    channel: Arc<NodeRequestChannel>,
    next_timer_id: Arc<AtomicU32>,
}

impl NodeRequestMainThr {
    pub fn request(&self, flags: NodeRequestFlags) {
        self.channel.request_flags.fetch_or(flags.bits(), Ordering::SeqCst);
    }

    #[cfg(feature = "external-plugin-guis")]
    pub fn request_gui_resize(&self, new_size: GuiSize) {
        if let Some(requested_gui_size) = &self.channel.requested_gui_size {
            requested_gui_size.store(new_size.to_u64(), Ordering::SeqCst);
        }
    }

    /// Request the host to register a timer for this node.
    ///
    /// This will return an error if this node did not specify that it
    /// wanted to use timers.
    pub fn register_timer(&self, period_ms: u32) -> Result<TimerID, ()> {
        if let Some(timer_requests) = &self.channel.timer_requests {
            let timer_id = TimerID(self.next_timer_id.load(Ordering::SeqCst));
            self.next_timer_id.store(timer_id.0 + 1, Ordering::SeqCst);

            // Using a mutex here is okay because the node can't make timer requests
            // from the realtime thread.
            let mut r = timer_requests.lock().unwrap();
            r.push(HostTimerRequest { timer_id, period_ms, register: true });

            Ok(timer_id)
        } else {
            Err(())
        }
    }

    /// Request the host to unregister a timer for this node.
    pub fn unregister_timer(&self, timer_id: TimerID) {
        if let Some(timer_requests) = &self.channel.timer_requests {
            // Using a mutex here is okay because the node can't make timer requests
            // from the realtime thread.
            let mut r = timer_requests.lock().unwrap();
            r.push(HostTimerRequest { timer_id, period_ms: 0, register: false });
        }
    }
}

#[derive(Clone)]
pub struct NodeRequestAudioThr {
    channel: Arc<NodeRequestChannel>,
}

impl NodeRequestAudioThr {
    pub fn request(&self, flags: NodeRequestFlags) {
        self.channel.request_flags.fetch_or(flags.bits(), Ordering::SeqCst);
    }

    #[cfg(feature = "external-plugin-guis")]
    pub fn request_gui_resize(&self, new_size: GuiSize) {
        if let Some(requested_gui_size) = &self.channel.requested_gui_size {
            requested_gui_size.store(new_size.to_u64(), Ordering::SeqCst);
        }
    }
}

struct NodeRequestChannel {
    request_flags: AtomicU64,

    #[cfg(feature = "external-plugin-guis")]
    requested_gui_size: Option<AtomicU64>, // GuiSize, No request = MAX

    timer_requests: Option<Mutex<Vec<HostTimerRequest>>>,
}

impl NodeRequestChannel {
    fn new(
        #[cfg(feature = "external-plugin-guis")] node_supports_gui: bool,
        node_supports_timers: bool,
    ) -> Self {
        #[cfg(feature = "external-plugin-guis")]
        let requested_gui_size =
            if node_supports_gui { Some(AtomicU64::new(u64::MAX)) } else { None };

        let timer_requests = if node_supports_timers { Some(Mutex::new(Vec::new())) } else { None };

        Self {
            request_flags: AtomicU64::new(NodeRequestFlags::empty().bits()),
            #[cfg(feature = "external-plugin-guis")]
            requested_gui_size,
            timer_requests,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostTimerRequest {
    pub timer_id: TimerID,
    pub period_ms: u32,

    /// `true` = register, `false` = unregister
    pub register: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gui_size_to_u64() {
        let gui_size = GuiSize { width: 111111, height: 222222 };
        let v = gui_size.to_u64();
        let gui_size_2 = GuiSize::from_u64(v);

        assert_eq!(gui_size, gui_size_2);
    }
}
