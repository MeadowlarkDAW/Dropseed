use bitflags::bitflags;

use super::StableID;

bitflags! {
    /// Bit flags describing the supported dialects of a note port.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct NoteDialect: u32 {
        /// Uses clap_event_note and clap_event_note_expression.
        const CLAP = 1 << 0;

        /// Uses midi, no polyphonic expression
        const MIDI = 1 << 1;

        /// Uses midi, with polyphonic expression (MPE)
        const MIDI_MPE = 1 << 2;

        /// Uses midi2
        const MIDI2 = 1 << 3;
    }
}

/// Information about a note port on a node.
#[derive(Debug, Clone)]
pub struct NotePortInfo {
    /// id identifies a port and must be stable.
    ///
    /// id may overlap between input and output ports.
    pub id: StableID,

    /// Bit flags describing the supported note dialects.
    pub supported_dialects: NoteDialect,

    /// The preferred note dialect (only one dialect).
    pub preferred_dialect: NoteDialect,

    /// The displayable name for this port.
    pub name: Option<String>,
}
