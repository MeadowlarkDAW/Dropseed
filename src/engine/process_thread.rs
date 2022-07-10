use basedrop::Owned;
use rtrb::{Consumer, Producer};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use crate::graph::schedule::SharedSchedule;

//static PROCESS_THREAD_POLL_INTERVAL: Duration = Duration::from_micros(5);

pub(crate) struct DSEngineProcessThread {
    to_audio_thread_audio_out_tx: Owned<Producer<f32>>,
    from_audio_thread_audio_in_rx: Owned<Consumer<f32>>,

    /// In case there are no inputs, use this to let the engine know when there
    /// are frames to render.
    num_frames_wanted: Option<Arc<AtomicU32>>,

    in_temp_buffer: Owned<Vec<f32>>,
    out_temp_buffer: Owned<Vec<f32>>,

    in_channels: usize,
    out_channels: usize,

    schedule: SharedSchedule,
}

impl DSEngineProcessThread {
    pub fn new(
        to_audio_thread_audio_out_tx: Owned<Producer<f32>>,
        from_audio_thread_audio_in_rx: Owned<Consumer<f32>>,
        num_frames_wanted: Option<Arc<AtomicU32>>,
        in_temp_buffer: Owned<Vec<f32>>,
        out_temp_buffer: Owned<Vec<f32>>,
        in_channels: usize,
        out_channels: usize,
        schedule: SharedSchedule,
    ) -> Self {
        Self {
            to_audio_thread_audio_out_tx,
            from_audio_thread_audio_in_rx,
            num_frames_wanted,
            in_temp_buffer,
            out_temp_buffer,
            in_channels,
            out_channels,
            schedule,
        }
    }

    pub fn run(&mut self, run: Arc<AtomicBool>) {
        // TODO: Use some kind of interrupt to activate the thread as apposed
        // to potentially pinning a whole CPU core just waiting for frames to
        // process?
        //
        // Note that I already tried the method of calling `thread::sleep()`
        // for a short amount of time while there is no work to be done, but
        // apparently Windows does not like it when you call `thread::sleep()`
        // on a high-priority thread (underruns galore).

        while run.load(Ordering::Relaxed) {
            let num_frames = if let Some(num_frames_wanted) = &self.num_frames_wanted {
                let num_frames = num_frames_wanted.load(Ordering::SeqCst);

                if num_frames == 0 {
                    continue;
                }

                num_frames as usize
            } else {
                let num_samples = self.from_audio_thread_audio_in_rx.slots();

                if num_samples == 0 {
                    continue;
                }

                let chunk = self.from_audio_thread_audio_in_rx.read_chunk(num_samples).unwrap();

                let (slice_1, slice_2) = chunk.as_slices();

                self.in_temp_buffer.clear();
                self.in_temp_buffer.extend_from_slice(slice_1);
                self.in_temp_buffer.extend_from_slice(slice_2);

                chunk.commit_all();

                num_samples / self.in_channels
            };

            self.out_temp_buffer.clear();
            self.out_temp_buffer.resize(num_frames * self.out_channels, 0.0);

            self.schedule.process_interleaved(
                &*self.in_temp_buffer,
                self.in_channels,
                &mut *self.out_temp_buffer,
                self.out_channels,
            );

            match self.to_audio_thread_audio_out_tx.write_chunk(num_frames * self.out_channels) {
                Ok(mut chunk) => {
                    let (slice_1, slice_2) = chunk.as_mut_slices();

                    let out_part = &self.out_temp_buffer[0..slice_1.len()];
                    for i in 0..slice_1.len() {
                        slice_1[i] = out_part[i];
                    }

                    let out_part =
                        &self.out_temp_buffer[slice_1.len()..slice_1.len() + slice_2.len()];
                    for i in 0..slice_2.len() {
                        slice_2[i] = out_part[i];
                    }

                    chunk.commit_all();
                }
                Err(_) => {
                    log::error!("Ran out of space in engine_to_audio_thread_audio_out buffer");
                    return;
                }
            }
        }
    }
}
