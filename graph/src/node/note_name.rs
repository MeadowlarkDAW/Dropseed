/// A custom name given to a particular note in a node.
#[derive(Debug, Clone)]
pub struct NoteName {
    /// The name of the note.
    pub name: String,

    /// The ID of the port
    ///
    /// Set to -1 for every port
    pub port: i16,

    /// The key
    ///
    /// Set to -1 for every key
    pub key: i16,

    /// The channel
    ///
    /// Set to -1 for every channel
    pub channel: i16,
}
