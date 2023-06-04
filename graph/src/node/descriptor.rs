#[derive(Debug, Clone)]
pub struct NodeDescriptor {
    /// The ID of the node (mandatory).
    ///
    /// The ID is an arbritrary string which should be unique to your node,
    /// It is encouraged to use a reverse URI eg: "app.meadowlark.spicysynth"
    pub id: String,

    /// The display name of this node (mandatory).
    pub name: String,

    /// The vendor of this node (optional).
    pub vendor: Option<String>,

    /// The URL to the node's product page (optional).
    ///
    /// eg: https://meadowlark.app
    pub url: Option<String>,

    /// The URL to the node's manual (optional).
    pub manual_url: Option<String>,

    /// The URL to the node's support page (optional).
    pub support_url: Option<String>,

    /// The version of this node (optional).
    ///
    /// It is useful for the host to understand and be able to compare two different
    /// version strings, so here is a regex like expression which is likely to be
    /// understood by most hosts: MAJOR(.MINOR(.REVISION)?)?( (Alpha|Beta) XREV)?
    pub version: Option<String>,

    /// A short description of this node (optional).
    pub description: Option<String>,

    /// An arbitrary list of keywords. They can be matched by the host indexer and
    /// used to classify the node.
    ///
    /// For some common features, see `features.rs`.
    ///
    /// Non-standard features should be formatted as follow: "$namespace:$feature".
    pub features: Vec<String>,

    /// What type of node this is. This lets the host know what version of the node's
    /// `process()` method it should call.
    pub node_type: NodeType,
}

impl Default for NodeDescriptor {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            vendor: None,
            url: None,
            manual_url: None,
            support_url: None,
            version: None,
            description: None,
            features: Vec::new(),
            node_type: NodeType::InternalRust,
        }
    }
}

/// What type of node this is. This lets the host know what version of the node's
/// `process()` method it should call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    /// An internal node written in Rust.
    InternalRust,

    #[cfg(feature = "c-bindings")]
    /// An internal node which uses the external C bindings.
    InternalC,

    #[cfg(feature = "clap-hosting")]
    /// An external CLAP plugin.
    ExternalCLAP,
}
