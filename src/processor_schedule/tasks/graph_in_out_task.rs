use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;
use basedrop::{Shared, Owned};
use smallvec::SmallVec;
use atomic_refcell::{AtomicRefCell, AtomicRefMut};
use rtrb::{Consumer, Producer};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc,
};

use crate::engine::audio_thread::AUDIO_THREAD_POLL_INTERVAL;

#[derive(Clone)]
pub(crate) struct NumFramesRequest {
    shared: Arc<AtomicU64>,
}

impl NumFramesRequest {
    pub fn new() -> Self {
        Self { shared: Arc::new(AtomicU64::new(0)) }
    }

    /// (frames requested, request version)
    pub fn load(&self) -> (u32, u32) {
        let val = self.shared.load(Ordering::SeqCst);

        (
            (val >> 32) as u32,
            (val & (u32::MAX as u64)) as u32,
        )
    }

    pub fn store(&mut self, frames_requested: u32, request_version: u32) {
        let mut val: u64 = 0;

        val = (frames_requested as u64) << 32;
        val += request_version as u64;

        self.shared.store(val, Ordering::SeqCst);
    }
}

pub(crate) struct GraphInTask {
    pub audio_out: SmallVec<[SharedBuffer<f32>; 4]>,
}

impl GraphInTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        // TODO: Collect inputs from audio thread.

        for shared_buffer in self.audio_out.iter() {
            shared_buffer.clear_until(proc_info.frames);
        }
    }
}

#[derive(Clone)]
pub struct SharedGraphInNode {
    shared: Shared<AtomicRefCell<GraphInNode>>,
}

impl SharedGraphInNode {
    pub fn new(n: GraphInNode, coll_handle: &basedrop::Handle) -> Self {
        Self {
            shared: Shared::new(coll_handle, AtomicRefCell::new(n)),
        }
    }

    pub fn borrow_mut<'a>(&'a self) -> AtomicRefMut<'a, GraphInNode> {
        self.shared.borrow_mut()
    }
}

pub struct GraphInNode {
    from_audio_thread_audio_in_rx: Owned<Consumer<f32>>,
    /// In case there are no inputs, use this to let the engine know when there
    /// are frames to render.
    num_frames_request: Option<NumFramesRequest>,
    last_request_version: u32,
    total_frames_requested: usize,
    run: Arc<AtomicBool>,
    in_temp_buffer: Owned<Vec<f32>>,
    num_audio_channels: usize,
    max_frames: usize,
}

impl GraphInNode {
    pub fn new(from_audio_thread_audio_in_rx: Owned<Consumer<f32>>, num_frames_request: Option<NumFramesRequest>, run: Arc<AtomicBool>, in_temp_buffer: Owned<Vec<f32>>, num_audio_channels: usize, max_frames: usize) -> Self {
        Self { from_audio_thread_audio_in_rx, num_frames_request, run, in_temp_buffer, num_audio_channels, max_frames, last_request_version: u32::MAX, total_frames_requested: 0 }
    }

    pub fn process(&mut self, audio_out: &mut [SharedBuffer<f32>]) -> Result<usize, ()> {
        assert_eq!(self.num_audio_channels, audio_out.len());

        let mut num_frames = 0;
        while self.run.load(Ordering::Relaxed) {
            if let Some(num_frames_request) = &self.num_frames_request {
                let (num_frames_wanted, request_version) = num_frames_request.load();
                if self.last_request_version != request_version {
                    self.last_request_version = request_version;
                    self.total_frames_requested += num_frames_wanted as usize;
                }

                if self.total_frames_requested == 0 {
                    #[cfg(not(target_os = "windows"))]
                    std::thread::sleep(AUDIO_THREAD_POLL_INTERVAL);

                    #[cfg(target_os = "windows")]
                    spin_sleeper.sleep(AUDIO_THREAD_POLL_INTERVAL);

                    continue;
                }

                num_frames = self.total_frames_requested.min(self.max_frames);
                self.total_frames_requested -= num_frames;

                break;
            } else {
                let num_samples = self.from_audio_thread_audio_in_rx.slots();

                if num_samples == 0 {
                    #[cfg(not(target_os = "windows"))]
                    std::thread::sleep(AUDIO_THREAD_POLL_INTERVAL);

                    #[cfg(target_os = "windows")]
                    spin_sleeper.sleep(AUDIO_THREAD_POLL_INTERVAL);

                    continue;
                }

                num_frames = (num_samples / self.num_audio_channels).min(self.max_frames);

                let chunk = self.from_audio_thread_audio_in_rx.read_chunk(num_frames * self.num_audio_channels).unwrap();

                let (slice_1, slice_2) = chunk.as_slices();

                self.in_temp_buffer.clear();
                self.in_temp_buffer.extend_from_slice(slice_1);
                self.in_temp_buffer.extend_from_slice(slice_2);

                chunk.commit_all();
                break;
            };
        }

        if num_frames == 0 {
            return Ok(0);
        }

        if self.num_audio_channels == 0 {
            return Ok(num_frames);
        }

        let in_buffer = &self.in_temp_buffer[0..(num_frames * self.num_audio_channels)];

        // De-interleave the input buffer into this node's output buffers.
        for (channel_i, audio_out_buffer) in audio_out.iter().enumerate() {
            let buffer = &mut audio_out_buffer.borrow_mut()[0..num_frames];

            for i in 0..num_frames {
                buffer[i] = in_buffer[(i * self.num_audio_channels) + channel_i];
            }
        }

        Ok(num_frames)
    }
}

pub(crate) struct GraphOutTask {
    pub audio_in: SmallVec<[SharedBuffer<f32>; 4]>,
}

impl GraphOutTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        // TODO: Send outputs to audio thread.
    }
}
