use bitflags::bitflags;
use clack_extensions::gui::GuiSize;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

bitflags! {
    /// A bitmask of all possible requests to make to the Host's main thread.
    ///
    /// The host is free to not fulfill the request at its own discretion.
    pub struct HostRequestFlags: u32 {
        /// The plugin requested its Audio Processor to be restarted
        const RESTART = 1 << 0;

        /// Should activate the plugin and start processing
        const PROCESS = 1 << 2;

        /// Should call the on_main() callback
        const CALLBACK = 1 << 3;

        /// Should rescan audio and note ports
        const RESCAN_PORTS = 1 << 4;

        /// Should rescan parameters
        const RESCAN_PARAMS = 1 << 5;

        /// Should flush parameter values
        const FLUSH_PARAMS = 1 << 6;

        /// Should resize the GUI
        const GUI_RESIZE = 1 << 7;

        /// Should update GUI resize hints
        const GUI_HINTS_CHANGED = 1 << 8;

        /// Should show the GUI
        const GUI_SHOW = 1 << 9;

        /// Should hide the GUI
        const GUI_HIDE = 1 << 10;

        /// Should register the user closed the floating UI
        const GUI_CLOSED = 1 << 11;

        /// Should register the connection to the UI was lost
        const GUI_DESTROYED = 1 << 12;

        /// The plugin has changed its state and it should be saved again.
        ///
        /// (Note that when a parameter value changes, it is implicit that
        /// the state is dirty and no there is no need to set this flag.)
        const MARK_DIRTY = 1 << 13;

        const TIMER_REQUEST = 1 << 14;
    }
}

/// The receiving end of the Host Request Channel.
///
/// The Host Request Channel is a bitmask-based MPSC communication channel that allows plugins to notify the main
/// thread that certain actions (see [`HostRequestFlags`]) are to be taken.
///
/// This channel **requires** said actions to be idempotent: it does not differentiate
/// between sending one and multiple requests until any of them are received.
///
/// This channel's actions are specific to a specific plugin instance: each plugin instance will
/// have its own channel.
pub struct HostRequestChannelReceiver {
    contents: Arc<HostChannelContents>,
    requested_timers: Arc<Mutex<Vec<HostTimerRequest>>>,
}

impl HostRequestChannelReceiver {
    pub fn new_channel(main_thread_id: std::thread::ThreadId) -> (Self, HostRequestChannelSender) {
        let contents = Arc::new(HostChannelContents::default());
        let requested_timers = Arc::new(Mutex::new(Vec::new()));

        (
            Self { contents: contents.clone(), requested_timers: Arc::clone(&requested_timers) },
            HostRequestChannelSender { contents, requested_timers, main_thread_id },
        )
    }

    /// Returns all the requests that have been made to the channel since the last call to [`fetch_requests`].
    ///
    /// This operation never blocks.
    #[inline]
    pub fn fetch_requests(&self) -> HostRequestFlags {
        HostRequestFlags::from_bits_truncate(
            self.contents.request_flags.swap(HostRequestFlags::empty().bits, Ordering::SeqCst),
        )
    }

    /// Returns the last GUI size that was requested (through a call to [`request_gui_resize`](HostRequestChannelSender::request_gui_resize)).
    ///
    /// This returns [`None`] if no new size has been requested for this plugin yet.
    #[inline]
    pub fn fetch_gui_size_request(&self) -> Option<GuiSize> {
        let size = GuiSize::from_u64(
            self.contents
                .last_gui_size_requested
                .swap(GuiSize { width: u32::MAX, height: u32::MAX }.to_u64(), Ordering::SeqCst),
        );

        match size {
            GuiSize { width: u32::MAX, height: u32::MAX } => None,
            size => Some(size),
        }
    }

    pub fn fetch_timer_requests(&mut self) -> Vec<HostTimerRequest> {
        let mut v = Vec::new();

        // Using a mutex here is realtime-safe because this is only used in the main
        // thread.
        let mut requested_timers = self.requested_timers.lock().unwrap();
        if !requested_timers.is_empty() {
            std::mem::swap(&mut *requested_timers, &mut v)
        }

        v
    }
}

/// The sender end of the Host Request Channel.
///
/// See the [`HostRequestChannelReceiver`] docs for more information about how this works.
///
/// Cloning this sender does not clone the underlying data: all cloned copies will be linked to the
/// same channel.
#[derive(Clone)]
pub struct HostRequestChannelSender {
    contents: Arc<HostChannelContents>,
    requested_timers: Arc<Mutex<Vec<HostTimerRequest>>>,
    main_thread_id: std::thread::ThreadId,
}

impl HostRequestChannelSender {
    pub fn request(&self, flags: HostRequestFlags) {
        self.contents.request_flags.fetch_or(flags.bits, Ordering::SeqCst);
    }

    pub fn request_gui_resize(&self, new_size: GuiSize) {
        self.contents.last_gui_size_requested.store(new_size.to_u64(), Ordering::SeqCst);
        self.request(HostRequestFlags::GUI_RESIZE)
    }

    /// Request the host to register a timer for this plugin.
    ///
    /// This can only be called on the main thread.
    pub fn register_timer(&mut self, period_ms: u32, timer_id: u32) {
        if std::thread::current().id() == self.main_thread_id {
            // Using a mutex here is realtime-safe because we only allow this mutex
            // to be used in the main thread.
            let mut requested_timers = self.requested_timers.lock().unwrap();
            requested_timers.push(HostTimerRequest { timer_id, period_ms, register: true });

            self.request(HostRequestFlags::TIMER_REQUEST);
        }
    }

    /// Request the host to unregister a timer for this plugin.
    ///
    /// This can only be called on the main thread.
    pub fn unregister_timer(&mut self, timer_id: u32) {
        // Using a mutex here is realtime-safe because we only allow this mutex
        // to be used in the main thread.
        if std::thread::current().id() == self.main_thread_id {
            let mut requested_timers = self.requested_timers.lock().unwrap();
            requested_timers.push(HostTimerRequest { timer_id, period_ms: 0, register: false });

            self.request(HostRequestFlags::TIMER_REQUEST);
        }
    }
}

struct HostChannelContents {
    request_flags: AtomicU32,           // HostRequestFlags
    last_gui_size_requested: AtomicU64, // GuiSize, default value (i.e. never requested) = MAX
}

impl Default for HostChannelContents {
    fn default() -> Self {
        Self {
            request_flags: AtomicU32::new(HostRequestFlags::empty().bits),
            last_gui_size_requested: AtomicU64::new(
                GuiSize { width: u32::MAX, height: u32::MAX }.to_u64(),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostTimerRequest {
    pub timer_id: u32,
    pub period_ms: u32,

    /// `true` = register, `false` = unregister
    pub register: bool,
}
