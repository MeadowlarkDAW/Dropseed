#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// Default setting, for "realtime" processing
    Realtime = 0,

    /// For processing without realtime pressure
    ///
    /// The node may use more expensive algorithms for higher sound quality.
    Offline = 1,
}
