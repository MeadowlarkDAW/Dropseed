use crate::{graph::shared_pool::SharedBuffer, ProcInfo};

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
        input: &SharedBuffer<f32>,
        output: &SharedBuffer<f32>,
    ) {
        // SAFETY
        // - These buffers are only ever borrowed in the audio thread.
        // - The schedule verifier has ensured that no data races can occur between parallel
        // audio threads due to aliasing buffer pointers.
        // - `proc_info.frames` will always be less than or equal to the allocated size of
        // all process audio buffers.
        let (input_ref, mut output_ref) = unsafe { (input.borrow(), output.borrow_mut()) };

        #[cfg(debug_assertions)]
        let (input, output) =
            (&input_ref[0..proc_info.frames], &mut output_ref[0..proc_info.frames]);
        #[cfg(not(debug_assertions))]
        let (input, output) = unsafe {
            (
                std::slice::from_raw_parts(input_ref.as_ptr(), proc_info.frames),
                std::slice::from_raw_parts_mut(output_ref.as_mut_ptr(), proc_info.frames),
            )
        };

        if proc_info.frames > self.buf.len() {
            if self.read_pointer == 0 {
                // Only one copy operation is needed.

                // Copy all frames from self.buf into the output buffer.
                output[0..self.buf.len()].copy_from_slice(&self.buf[0..self.buf.len()]);
            } else if self.read_pointer < self.buf.len() {
                // This check will always be true, it is here to hint to the compiler to optimize.
                // Two copy operations are needed.

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
                // Only one copy operation is needed.

                // Copy frames from self.buf into the output buffer.
                output[0..proc_info.frames].copy_from_slice(
                    &self.buf[self.read_pointer..self.read_pointer + proc_info.frames],
                );

                // Copy all frames from the input buffer into self.buf.
                self.buf[self.read_pointer..self.read_pointer + proc_info.frames]
                    .copy_from_slice(&input[0..proc_info.frames]);
            } else {
                // Two copy operations are needed.

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
