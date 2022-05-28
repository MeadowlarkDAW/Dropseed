//! The code in this file is ported from:
//!
//! https://github.com/free-audio/clap-helpers/blob/main/include/clap/helpers/reducing-param-queue.hh
//! https://github.com/free-audio/clap-helpers/blob/main/include/clap/helpers/reducing-param-queue.hxx
//!
//! MIT License
//!
//! Copyright (c) 2021 Alexandre BIQUE
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.

// TODO: Validate the safety of this algorithm.

use basedrop::Shared;
use fnv::FnvHashMap;
use std::cell::UnsafeCell;
use std::hash::Hash;

pub trait ReducingQueueValue: Clone + Send + Sync + 'static {
    #[allow(unused)]
    fn update(&mut self, new: &Self) {}
}

struct ReducingQueue<K: Send + Sync + Eq + Hash + 'static, V: ReducingQueueValue> {
    queues: [FnvHashMap<K, V>; 2],
    free: Option<usize>,
    producer: usize,
    consumer: Option<usize>,
}

pub fn reducing_queue<K: Send + Sync + Eq + Hash + 'static, V: ReducingQueueValue>(
    capacity: usize,
    coll_handle: &basedrop::Handle,
) -> (RQProducer<K, V>, RQConsumer<K, V>) {
    let mut queues = [FnvHashMap::default(), FnvHashMap::default()];

    queues[0].reserve(capacity * 2);
    queues[1].reserve(capacity * 2);

    let free = Some(0);
    let producer = 1;

    let shared = Shared::new(
        coll_handle,
        UnsafeCell::new(ReducingQueue { queues, free, producer, consumer: None }),
    );

    (RQProducer { shared: Shared::clone(&shared) }, RQConsumer { shared })
}

pub struct RQProducer<K: Send + Sync + Eq + Hash + 'static, V: ReducingQueueValue> {
    shared: Shared<UnsafeCell<ReducingQueue<K, V>>>,
}

impl<K: Send + Sync + Eq + Hash + 'static, V: ReducingQueueValue> RQProducer<K, V> {
    pub fn set(&mut self, key: K, value: V) {
        let shared = unsafe { &mut *self.shared.get() };

        let prod = unsafe { shared.queues.get_unchecked_mut(shared.producer) };

        let _ = prod.insert(key, value);
    }

    pub fn set_or_update(&mut self, key: K, value: V) {
        let shared = unsafe { &mut *self.shared.get() };

        let prod = unsafe { shared.queues.get_unchecked_mut(shared.producer) };

        if let Some(v) = prod.get_mut(&key) {
            v.update(&value);
        } else {
            let _ = prod.insert(key, value);
        }
    }

    pub fn producer_done(&mut self) {
        let shared = unsafe { &mut *self.shared.get() };

        if shared.consumer.is_some() {
            return;
        }

        let tmp = shared.producer;
        shared.producer = shared.free.unwrap();
        shared.free = None;
        shared.consumer = Some(tmp);
    }
}

unsafe impl<K: Send + Sync + Eq + Hash + 'static, V: ReducingQueueValue> Send for RQProducer<K, V> {}
unsafe impl<K: Send + Sync + Eq + Hash + 'static, V: ReducingQueueValue> Sync for RQProducer<K, V> {}

pub struct RQConsumer<K: Send + Sync + Eq + Hash + 'static, V: ReducingQueueValue> {
    shared: Shared<UnsafeCell<ReducingQueue<K, V>>>,
}

impl<K: Send + Sync + Eq + Hash + 'static, V: ReducingQueueValue> RQConsumer<K, V> {
    pub fn consume<F: FnMut(&K, &V)>(&mut self, mut f: F) {
        let shared = unsafe { &mut *self.shared.get() };

        if let Some(c) = shared.consumer.take() {
            let consumer = unsafe { shared.queues.get_unchecked_mut(c) };

            for (key, value) in consumer.iter() {
                (f)(key, value);
            }

            consumer.clear();

            if shared.free.is_some() {
                return;
            }

            shared.free = Some(c);
        }
    }
}

unsafe impl<K: Send + Sync + Eq + Hash + 'static, V: ReducingQueueValue> Send for RQConsumer<K, V> {}
unsafe impl<K: Send + Sync + Eq + Hash + 'static, V: ReducingQueueValue> Sync for RQConsumer<K, V> {}
