use bitflags::bitflags;
use clack_extensions::gui::GuiSize;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

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

        /// Should rescan audio ports
        const RESCAN_AUDIO_PORTS = 1 << 4;

        /// Should rescan note ports
        const RESCAN_NOTE_PORTS = 1 << 5;

        /// Should flush parameter values
        const FLUSH_PARAMS = 1 >> 6;

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
}

impl HostRequestChannelReceiver {
    pub fn new_channel() -> (Self, HostRequestChannelSender) {
        let contents = Arc::new(HostChannelContents::default());
        (Self { contents: contents.clone() }, HostRequestChannelSender { contents })
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
    /// Calling this method does not clear the last GUI size request. See
    /// [`clear_gui_size_requested`](HostRequestChannelReceiver::clear_gui_size_requested)) to do this.
    ///
    /// This returns [`None`] if no new size has been requested for this plugin yet.
    #[inline]
    pub fn last_gui_size_requested(&self) -> Option<GuiSize> {
        let size = GuiSize::from_u64(self.contents.last_gui_size_requested.load(Ordering::SeqCst));

        match size {
            GuiSize { width: u32::MAX, height: u32::MAX } => None,
            size => Some(size),
        }
    }

    /// Clears any previously requested GUI size.
    ///
    /// This method should be called whenever the UI was destroyed or closed, as its previously know
    /// size may not make sense for the new window.
    #[inline]
    pub fn clear_gui_size_requested(&self) {
        self.contents
            .last_gui_size_requested
            .store(GuiSize { width: u32::MAX, height: u32::MAX }.to_u64(), Ordering::SeqCst);
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
}

impl HostRequestChannelSender {
    #[inline]
    pub fn request(&self, flags: HostRequestFlags) {
        self.contents.request_flags.fetch_or(flags.bits, Ordering::SeqCst);
    }

    #[inline]
    pub fn request_gui_resize(&self, new_size: GuiSize) {
        self.contents.last_gui_size_requested.store(new_size.to_u64(), Ordering::SeqCst);
        self.request(HostRequestFlags::GUI_RESIZE)
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
