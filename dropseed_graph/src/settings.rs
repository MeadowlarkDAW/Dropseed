/// The settings for constructing a new Dropseed graph
#[derive(Debug, Clone, Copy)]
pub struct DsGraphSettings {
    /// The sample rate of the project.
    /// 
    /// Default is `44100.0`.
    pub sample_rate: f64,

    /// The minimum number of frames (samples in a single audio channel)
    /// the can be in a single process cycle.
    /// 
    /// If the given number is 0, then 1 will be used.
    /// 
    /// If `min_frames` > `max_frames`, then the value in `max_frames` will
    /// be used instead.
    /// 
    /// Default is `1`.
    pub min_frames: u32,

    /// The maximum number of frames (samples in a single audio channel)
    /// the can be in a single process cycle.
    /// 
    /// Default is `1024`.
    pub max_frames: u32,

    /// The total number of input audio channels to the audio graph.
    /// 
    /// Default is `1`
    pub num_audio_in_channels: u16,

    /// The total number of output audio channels from the audio graph.
    /// 
    /// Default is `2`
    pub num_audio_out_channels: u16,

    /// The pre-allocated capacity for the message channel inside the audio
    /// graph.
    /// 
    /// Minimum is `16`.
    ///
    /// Default is `256`.
    pub channel_size: usize,

    /// The pre-allocated capacity for note buffers in the audio graph.
    ///
    /// Default is `256`.
    pub note_buffer_size: usize,

    /// The pre-allocated capacity for parameter event buffers in the audio
    /// graph.
    ///
    /// Default is `256`.
    pub event_buffer_size: usize,
}

impl Default for DsGraphSettings {
    fn default() -> Self {
        Self {
            sample_rate: 44100.0,
            min_frames: 1,
            max_frames: 1024,
            num_audio_in_channels: 1,
            num_audio_out_channels: 2,
            channel_size: 256,
            note_buffer_size: 256,
            event_buffer_size: 256,
        }
    }
}