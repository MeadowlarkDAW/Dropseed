use crate::{graph::audio_buffer_pool::SharedAudioBuffer, ProcInfo};

pub(crate) struct DelayCompNode {
    buf: Vec<f32>,
    read_pointer: usize,
}

impl DelayCompNode {
    pub fn new(delay: u32) -> Self {
        Self { buf: vec![0.0; delay as usize], read_pointer: 0 }
    }

    pub fn process(
        &mut self,
        proc_info: &ProcInfo,
        input: &SharedAudioBuffer<f32>,
        output: &SharedAudioBuffer<f32>,
    ) {
        // Please refer to the "SAFETY NOTE" at the top of the file
        // `src/graph/audio_buffer_pool.rs` on why it is considered safe to
        // borrow these buffers.
        //
        // In addition the host will never set `proc_info.frames` to something
        // higher than the maximum frame size (which is what the Vec's initial
        // capacity is set to).
        let (input, output) = unsafe { (input.borrow(proc_info), output.borrow_mut(proc_info)) };

        if proc_info.frames > self.buf.len() {
            if self.read_pointer == 0 {
                // Only one copy is needed.

                // Copy all frames from self.buf into the output buffer.
                output[0..self.buf.len()].copy_from_slice(&self.buf[0..self.buf.len()]);
            } else if self.read_pointer < self.buf.len() {
                // This check will always be true, it is here to hint to the compiler to optimize.
                // Two copies are needed.

                let first_len = self.buf.len() - self.read_pointer;

                // Copy frames from self.buf into the output buffer.
                output[0..first_len].copy_from_slice(&self.buf[self.read_pointer..self.buf.len()]);
                output[first_len..self.buf.len()].copy_from_slice(&self.buf[0..self.read_pointer]);
            }

            // Copy the remaining frames from the input buffer to the output buffer.
            let remaining = proc_info.frames - self.buf.len();
            output[self.buf.len()..proc_info.frames].copy_from_slice(&input[0..remaining]);

            // Copy the final remaining frames from the input buffer into self.buf.
            // self.buf is "empty" at this point, so reset the read pointer so only one copy operation is needed.
            self.read_pointer = 0;
            let buf_len = self.buf.len();
            self.buf[0..buf_len].copy_from_slice(&input[remaining..proc_info.frames]);
        } else {
            if self.read_pointer + proc_info.frames <= self.buf.len() {
                // Only one copy is needed.

                // Copy frames from self.buf into the output buffer.
                output[0..proc_info.frames].copy_from_slice(
                    &self.buf[self.read_pointer..self.read_pointer + proc_info.frames],
                );

                // Copy all frames from the input buffer into self.buf.
                self.buf[self.read_pointer..self.read_pointer + proc_info.frames]
                    .copy_from_slice(&input[0..proc_info.frames]);
            } else {
                // Two copies are needed.

                let first_len = self.buf.len() - self.read_pointer;
                let second_len = proc_info.frames - first_len;

                // Copy frames from self.buf into the output buffer.
                output[0..first_len].copy_from_slice(&self.buf[self.read_pointer..self.buf.len()]);
                output[first_len..proc_info.frames].copy_from_slice(&self.buf[0..second_len]);

                // Copy all frames from the input buffer into self.buf.
                let buf_len = self.buf.len();
                self.buf[self.read_pointer..buf_len].copy_from_slice(&input[0..first_len]);
                self.buf[0..second_len].copy_from_slice(&input[first_len..proc_info.frames]);
            }

            // Get the next position of the read pointer.
            self.read_pointer += proc_info.frames;
            if self.read_pointer >= self.buf.len() {
                self.read_pointer -= self.buf.len();
            }
        }
    }

    pub fn delay(&self) -> u32 {
        self.buf.len() as u32
    }
}
