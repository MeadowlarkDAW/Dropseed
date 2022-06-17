//! This extension provides a way for the plugin to describe its current note ports.
//! If the plugin does not implement this extension, it won't have note input or output.
//! The plugin is only allowed to change its note ports configuration while it is deactivated.

use bitflags::bitflags;

use clap_sys::ext::note_ports::{
    CLAP_NOTE_DIALECT_CLAP, CLAP_NOTE_DIALECT_MIDI, CLAP_NOTE_DIALECT_MIDI2,
    CLAP_NOTE_DIALECT_MIDI_MPE, CLAP_NOTE_PORTS_RESCAN_ALL, CLAP_NOTE_PORTS_RESCAN_NAMES,
};

pub(crate) static EMPTY_NOTE_PORTS_CONFIG: PluginNotePortsExt = PluginNotePortsExt::empty();

bitflags! {
    pub struct NoteDialect: u32 {
        /// Uses clap_event_note and clap_event_note_expression.
        const CLAP = CLAP_NOTE_DIALECT_CLAP;
        /// Uses clap_event_midi, no polyphonic expression
        const MIDI = CLAP_NOTE_DIALECT_MIDI;
        /// Uses clap_event_midi, with polyphonic expression (MPE)
        const MIDI_MPE = CLAP_NOTE_DIALECT_MIDI_MPE;
        /// Uses clap_event_midi2
        const MIDI2 = CLAP_NOTE_DIALECT_MIDI2;
    }
}

#[derive(Debug, Clone, PartialEq)]
/// The layout of the audio ports of a plugin.
pub struct PluginNotePortsExt {
    /// The list of input note ports, in order.
    pub inputs: Vec<NotePortInfo>,

    /// The list of output note ports, in order.
    pub outputs: Vec<NotePortInfo>,
}

impl PluginNotePortsExt {
    pub const fn empty() -> Self {
        Self { inputs: Vec::new(), outputs: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotePortInfo {
    /// stable identifier
    pub stable_id: u32,

    /// bitfield, see `NoteDialect`
    pub supported_dialects: NoteDialect,

    /// one value of `NoteDialect`
    pub preferred_dialect: NoteDialect,

    /// displayable name
    pub display_name: Option<String>,
}

bitflags! {
    pub struct NotePortRescanFlags: u32 {
        /// The ports have changed, the host shall perform a full scan of the ports.
        ///
        /// This flag can only be used if the plugin is not active.
        ///
        /// If the plugin active, call host_request.request_restart() and then call rescan()
        /// when the host calls deactivate()
        const RESCAN_ALL = CLAP_NOTE_PORTS_RESCAN_ALL;

        /// The ports name did change, the host can scan them right away.
        const RESCAN_NAMES = CLAP_NOTE_PORTS_RESCAN_NAMES;
    }
}
