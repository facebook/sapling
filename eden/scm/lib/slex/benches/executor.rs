/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::VecDeque;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use minibench::bench;
use minibench::elapsed;
use slex::Items;
use slex::Work;
use slex::WorkOptions;
use slex::WorkScope;
use slex::WorkShape;

const SIZES: [usize; 9] = [1, 2, 4, 8, 16, 32, 64, 256, 1024];
const CHANNEL_CAPACITIES: [usize; 15] = [
    1, 2, 4, 8, 16, 32, 64, 80, 128, 256, 512, 1024, 4096, 10_000, 100_000,
];
const CHANNEL_ALLOCATION_TARGET_SLOTS: usize = 4_000_000;
const CHANNEL_ALLOCATION_MIN_ITERS: usize = 100;
const CHANNEL_ALLOCATION_MAX_ITERS: usize = 100_000;
const BATCH_BUFFER_SIZES: [usize; 4] = [8, 16, 32, 5_000];
const BATCH_BUFFER_TARGET_ITEMS: usize = 2_000_000;
const BATCH_BUFFER_MIN_BATCHES: usize = 400;
const BATCH_BUFFER_MAX_BATCHES: usize = 200_000;
const SINGLE_BATCH_TARGET_ITEMS: usize = 2_000_000;
const SINGLE_BATCH_MIN_ITERS: usize = 400;
const SINGLE_BATCH_MAX_ITERS: usize = 200_000;
const WORKSET_ITEMS: [usize; 3] = [256, 4096, 32768];
const WORKSET_WORKERS: [usize; 3] = [1, 4, 8];
const WORKSET_SPAWN_BATCHES: [usize; 5] = [1, 2, 4, 8, 16];
const WORKSET_SPAWN_WORKERS: [usize; 3] = [1, 4, 8];
const WORKSET_TRANSITION_ITEMS: [usize; 9] = [1, 2, 4, 8, 16, 32, 64, 256, 1024];
const QUEUE_THROUGHPUT_ITEMS: usize = 1_000_000;
const QUEUE_THROUGHPUT_CAPACITIES: [usize; 3] = [1, 100, 1024];

fn cpu_work(mut value: u64) -> u64 {
    for _ in 0..2_000 {
        value = value
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
    }
    value
}

fn tiny_work(value: u64) -> u64 {
    value.wrapping_mul(31).wrapping_add(7)
}

fn byte_work(bytes: usize) -> usize {
    let mut data = vec![0u8; bytes];
    for (index, byte) in data.iter_mut().enumerate() {
        *byte = index as u8;
    }
    data.iter().map(|byte| *byte as usize).sum()
}

fn run_inline(size: usize, f: impl Fn(usize) -> usize) -> Vec<usize> {
    (0..size).map(f).collect()
}

fn run_parallel(
    options: WorkOptions,
    size: usize,
    f: impl Fn(usize) -> usize + Send + Sync + 'static,
) {
    let input: Items<usize, ()> = Items::ready((0..size).collect::<Vec<_>>());
    let results = Work::map(options, input, f);
    black_box(results);
}

fn channel_allocation_iters(capacity: usize) -> usize {
    if capacity <= 1 {
        return CHANNEL_ALLOCATION_MAX_ITERS;
    }
    (CHANNEL_ALLOCATION_TARGET_SLOTS / capacity)
        .clamp(CHANNEL_ALLOCATION_MIN_ITERS, CHANNEL_ALLOCATION_MAX_ITERS)
}

fn bench_channel_allocation() {
    bench(
        "channel alloc crossfire mpsc unbounded iters=100000",
        || {
            elapsed(|| {
                for _ in 0..CHANNEL_ALLOCATION_MAX_ITERS {
                    let (tx, rx) = crossfire::mpsc::unbounded_blocking::<usize>();
                    black_box((tx, rx));
                }
            })
        },
    );

    for capacity in CHANNEL_CAPACITIES {
        let iters = channel_allocation_iters(capacity);
        bench(
            format!("channel alloc crossfire mpsc bounded cap={capacity} iters={iters}"),
            move || {
                elapsed(move || {
                    for _ in 0..iters {
                        let (tx, rx) = crossfire::mpsc::bounded_blocking::<usize>(capacity);
                        black_box((tx, rx));
                    }
                })
            },
        );
    }
}

fn run_flume_blocking_queue(capacity: usize, items: usize) -> usize {
    let (tx, rx) = flume::bounded(capacity);
    let consumer = thread::spawn(move || {
        let mut checksum = 0usize;
        for _ in 0..items {
            checksum ^= rx.recv().expect("recv failed");
        }
        checksum
    });

    for item in 0..items {
        tx.send(item).expect("send failed");
    }

    consumer.join().expect("consumer panicked")
}

fn run_crossfire_blocking_queue(capacity: usize, items: usize) -> usize {
    let (tx, rx) = crossfire::mpsc::bounded_blocking(capacity);
    let consumer = thread::spawn(move || {
        let mut checksum = 0usize;
        for _ in 0..items {
            checksum ^= rx.recv().expect("recv failed");
        }
        checksum
    });

    for item in 0..items {
        tx.send(item).expect("send failed");
    }

    consumer.join().expect("consumer panicked")
}

fn run_flume_blocking_to_async_queue(capacity: usize, items: usize) -> usize {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build runtime");
    let (tx, rx) = flume::bounded(capacity);
    let producer = thread::spawn(move || {
        for item in 0..items {
            tx.send(item).expect("send failed");
        }
    });

    let checksum = runtime.block_on(async move {
        let mut checksum = 0usize;
        for _ in 0..items {
            checksum ^= rx.recv_async().await.expect("recv failed");
        }
        checksum
    });

    producer.join().expect("producer panicked");
    checksum
}

fn run_crossfire_blocking_to_async_queue(capacity: usize, items: usize) -> usize {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build runtime");
    let (tx, rx) = crossfire::mpsc::bounded_blocking_async(capacity);
    let producer = thread::spawn(move || {
        for item in 0..items {
            tx.send(item).expect("send failed");
        }
    });

    let checksum = runtime.block_on(async move {
        let mut checksum = 0usize;
        for _ in 0..items {
            checksum ^= rx.recv().await.expect("recv failed");
        }
        checksum
    });

    producer.join().expect("producer panicked");
    checksum
}

fn run_flume_try_send_to_async_queue(capacity: usize, items: usize) -> usize {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build runtime");
    let (tx, rx) = flume::bounded(capacity);
    let producer = thread::spawn(move || {
        let mut item = 0usize;
        while item < items {
            match tx.try_send(item) {
                Ok(()) => item += 1,
                Err(flume::TrySendError::Full(returned)) => {
                    item = returned;
                    while tx.is_full() {
                        thread::yield_now();
                    }
                }
                Err(flume::TrySendError::Disconnected(_)) => panic!("send failed"),
            }
        }
    });

    let checksum = runtime.block_on(async move {
        let mut checksum = 0usize;
        for _ in 0..items {
            checksum ^= rx.recv_async().await.expect("recv failed");
        }
        checksum
    });

    producer.join().expect("producer panicked");
    checksum
}

fn run_crossfire_try_send_to_async_queue(capacity: usize, items: usize) -> usize {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build runtime");
    let (tx, rx) = crossfire::mpsc::bounded_blocking_async(capacity);
    let producer = thread::spawn(move || {
        let mut item = 0usize;
        while item < items {
            match tx.try_send(item) {
                Ok(()) => item += 1,
                Err(crossfire::TrySendError::Full(returned)) => {
                    item = returned;
                    while tx.is_full() {
                        thread::yield_now();
                    }
                }
                Err(crossfire::TrySendError::Disconnected(_)) => panic!("send failed"),
            }
        }
    });

    let checksum = runtime.block_on(async move {
        let mut checksum = 0usize;
        for _ in 0..items {
            checksum ^= rx.recv().await.expect("recv failed");
        }
        checksum
    });

    producer.join().expect("producer panicked");
    checksum
}

fn run_flume_async_queue(capacity: usize, items: usize) -> usize {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build runtime");
    runtime.block_on(async move {
        let (tx, rx) = flume::bounded(capacity);
        let producer = async move {
            for item in 0..items {
                tx.send_async(item).await.expect("send failed");
            }
        };
        let consumer = async move {
            let mut checksum = 0usize;
            for _ in 0..items {
                checksum ^= rx.recv_async().await.expect("recv failed");
            }
            checksum
        };
        let ((), checksum) = tokio::join!(producer, consumer);
        checksum
    })
}

fn run_crossfire_async_queue(capacity: usize, items: usize) -> usize {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build runtime");
    runtime.block_on(async move {
        let (tx, rx) = crossfire::mpsc::bounded_async(capacity);
        let producer = async move {
            for item in 0..items {
                tx.send(item).await.expect("send failed");
            }
        };
        let consumer = async move {
            let mut checksum = 0usize;
            for _ in 0..items {
                checksum ^= rx.recv().await.expect("recv failed");
            }
            checksum
        };
        let ((), checksum) = tokio::join!(producer, consumer);
        checksum
    })
}

fn bench_queue_throughput() {
    for capacity in QUEUE_THROUGHPUT_CAPACITIES {
        bench(
            format!("queue flume blocking bounded cap={capacity} items={QUEUE_THROUGHPUT_ITEMS}"),
            move || {
                elapsed(move || {
                    black_box(run_flume_blocking_queue(capacity, QUEUE_THROUGHPUT_ITEMS));
                })
            },
        );
        bench(
            format!(
                "queue crossfire blocking bounded cap={capacity} items={QUEUE_THROUGHPUT_ITEMS}"
            ),
            move || {
                elapsed(move || {
                    black_box(run_crossfire_blocking_queue(
                        capacity,
                        QUEUE_THROUGHPUT_ITEMS,
                    ));
                })
            },
        );
        bench(
            format!(
                "queue flume blocking-to-async bounded cap={capacity} items={QUEUE_THROUGHPUT_ITEMS}"
            ),
            move || {
                elapsed(move || {
                    black_box(run_flume_blocking_to_async_queue(
                        capacity,
                        QUEUE_THROUGHPUT_ITEMS,
                    ));
                })
            },
        );
        bench(
            format!(
                "queue crossfire blocking-to-async bounded cap={capacity} items={QUEUE_THROUGHPUT_ITEMS}"
            ),
            move || {
                elapsed(move || {
                    black_box(run_crossfire_blocking_to_async_queue(
                        capacity,
                        QUEUE_THROUGHPUT_ITEMS,
                    ));
                })
            },
        );
        bench(
            format!(
                "queue flume try-send-to-async bounded cap={capacity} items={QUEUE_THROUGHPUT_ITEMS}"
            ),
            move || {
                elapsed(move || {
                    black_box(run_flume_try_send_to_async_queue(
                        capacity,
                        QUEUE_THROUGHPUT_ITEMS,
                    ));
                })
            },
        );
        bench(
            format!(
                "queue crossfire try-send-to-async bounded cap={capacity} items={QUEUE_THROUGHPUT_ITEMS}"
            ),
            move || {
                elapsed(move || {
                    black_box(run_crossfire_try_send_to_async_queue(
                        capacity,
                        QUEUE_THROUGHPUT_ITEMS,
                    ));
                })
            },
        );
        bench(
            format!("queue flume async bounded cap={capacity} items={QUEUE_THROUGHPUT_ITEMS}"),
            move || {
                elapsed(move || {
                    black_box(run_flume_async_queue(capacity, QUEUE_THROUGHPUT_ITEMS));
                })
            },
        );
        bench(
            format!("queue crossfire async bounded cap={capacity} items={QUEUE_THROUGHPUT_ITEMS}"),
            move || {
                elapsed(move || {
                    black_box(run_crossfire_async_queue(capacity, QUEUE_THROUGHPUT_ITEMS));
                })
            },
        );
    }
}

fn batch_buffer_iters(batch_size: usize) -> usize {
    (BATCH_BUFFER_TARGET_ITEMS / batch_size)
        .clamp(BATCH_BUFFER_MIN_BATCHES, BATCH_BUFFER_MAX_BATCHES)
}

fn fill_batch_buffer(buffer: &mut Vec<usize>, batch_size: usize, seed: &mut usize) {
    for _ in 0..batch_size {
        buffer.push(*seed);
        *seed = seed.wrapping_add(1);
    }
}

fn consume_batch(batch: Vec<usize>) -> usize {
    let len = batch.len();
    let cap = batch.capacity();
    let first = batch.first().copied().unwrap_or_default();
    let last = batch.last().copied().unwrap_or_default();
    black_box((len, cap, first, last));
    len
}

fn run_batch_take_new(batch_size: usize, batches: usize) -> usize {
    let mut buffer = Vec::new();
    let mut seed = 0usize;
    let mut total = 0usize;
    for _ in 0..batches {
        fill_batch_buffer(&mut buffer, batch_size, &mut seed);
        total += consume_batch(std::mem::take(&mut buffer));
    }
    black_box(buffer.capacity());
    total
}

fn run_batch_take_reset(batch_size: usize, batches: usize, reset_capacity: usize) -> usize {
    let mut buffer = Vec::with_capacity(reset_capacity);
    let mut seed = 0usize;
    let mut total = 0usize;
    for _ in 0..batches {
        fill_batch_buffer(&mut buffer, batch_size, &mut seed);
        total += consume_batch(std::mem::take(&mut buffer));
        buffer = Vec::with_capacity(reset_capacity);
    }
    black_box(buffer.capacity());
    total
}

#[expect(
    clippy::drain_collect,
    reason = "benchmark intentionally measures drain-and-collect batching"
)]
fn run_batch_drain_scratch(batch_size: usize, batches: usize, initial_capacity: usize) -> usize {
    let mut buffer = Vec::with_capacity(initial_capacity);
    let mut seed = 0usize;
    let mut total = 0usize;
    for _ in 0..batches {
        fill_batch_buffer(&mut buffer, batch_size, &mut seed);
        total += consume_batch(buffer.drain(..).collect::<Vec<_>>());
    }
    black_box(buffer.capacity());
    total
}

#[expect(
    clippy::drain_collect,
    reason = "benchmark intentionally compares reusable scratch against take"
)]
fn run_batch_hybrid_scratch(batch_size: usize, batches: usize, initial_capacity: usize) -> usize {
    let mut buffer = Vec::with_capacity(initial_capacity);
    let mut seed = 0usize;
    let mut total = 0usize;
    for batch_index in 0..batches {
        fill_batch_buffer(&mut buffer, batch_size, &mut seed);
        let batch = if batch_index + 1 == batches {
            std::mem::take(&mut buffer)
        } else {
            buffer.drain(..).collect()
        };
        total += consume_batch(batch);
    }
    black_box(buffer.capacity());
    total
}

fn single_batch_iters(batch_size: usize) -> usize {
    (SINGLE_BATCH_TARGET_ITEMS / batch_size).clamp(SINGLE_BATCH_MIN_ITERS, SINGLE_BATCH_MAX_ITERS)
}

fn run_single_batch_take_new(batch_size: usize, iters: usize) -> usize {
    let mut seed = 0usize;
    let mut total = 0usize;
    for _ in 0..iters {
        let mut buffer = Vec::new();
        fill_batch_buffer(&mut buffer, batch_size, &mut seed);
        total += consume_batch(std::mem::take(&mut buffer));
        black_box(buffer.capacity());
    }
    total
}

fn run_single_batch_take_reset(batch_size: usize, iters: usize, capacity: usize) -> usize {
    let mut seed = 0usize;
    let mut total = 0usize;
    for _ in 0..iters {
        let mut buffer = Vec::with_capacity(capacity);
        fill_batch_buffer(&mut buffer, batch_size, &mut seed);
        total += consume_batch(std::mem::take(&mut buffer));
        black_box(buffer.capacity());
    }
    total
}

#[expect(
    clippy::drain_collect,
    reason = "benchmark intentionally measures drain-and-collect batching"
)]
fn run_single_batch_drain_scratch(batch_size: usize, iters: usize, capacity: usize) -> usize {
    let mut seed = 0usize;
    let mut total = 0usize;
    for _ in 0..iters {
        let mut buffer = Vec::with_capacity(capacity);
        fill_batch_buffer(&mut buffer, batch_size, &mut seed);
        total += consume_batch(buffer.drain(..).collect::<Vec<_>>());
        black_box(buffer.capacity());
    }
    total
}

fn bench_batch_buffers() {
    for batch_size in BATCH_BUFFER_SIZES {
        let batches = batch_buffer_iters(batch_size);

        bench(
            format!("batch vec take-new batch={batch_size} batches={batches}"),
            move || {
                elapsed(move || {
                    black_box(run_batch_take_new(batch_size, batches));
                })
            },
        );

        let mut reset_capacities = vec![8, 16];
        if !reset_capacities.contains(&batch_size) {
            reset_capacities.push(batch_size);
        }
        for reset_capacity in reset_capacities {
            bench(
                format!(
                    "batch vec take-reset-cap={reset_capacity} batch={batch_size} batches={batches}"
                ),
                move || {
                    elapsed(move || {
                        black_box(run_batch_take_reset(batch_size, batches, reset_capacity));
                    })
                },
            );
        }

        let mut scratch_capacities = vec![0, 8, 16];
        if !scratch_capacities.contains(&batch_size) {
            scratch_capacities.push(batch_size);
        }
        for initial_capacity in scratch_capacities {
            bench(
                format!(
                    "batch vec drain-scratch-cap={initial_capacity} batch={batch_size} batches={batches}"
                ),
                move || {
                    elapsed(move || {
                        black_box(run_batch_drain_scratch(
                            batch_size,
                            batches,
                            initial_capacity,
                        ));
                    })
                },
            );
            bench(
                format!(
                    "batch vec hybrid-scratch-cap={initial_capacity} batch={batch_size} batches={batches}"
                ),
                move || {
                    elapsed(move || {
                        black_box(run_batch_hybrid_scratch(
                            batch_size,
                            batches,
                            initial_capacity,
                        ));
                    })
                },
            );
        }
    }
}

fn bench_batch_buffers_single() {
    for batch_size in BATCH_BUFFER_SIZES {
        let iters = single_batch_iters(batch_size);
        bench(
            format!("batch vec single take-new batch={batch_size} iters={iters}"),
            move || {
                elapsed(move || {
                    black_box(run_single_batch_take_new(batch_size, iters));
                })
            },
        );

        let mut reset_capacities = vec![8, 16];
        if !reset_capacities.contains(&batch_size) {
            reset_capacities.push(batch_size);
        }
        for reset_capacity in reset_capacities {
            bench(
                format!(
                    "batch vec single take-reset-cap={reset_capacity} batch={batch_size} iters={iters}"
                ),
                move || {
                    elapsed(move || {
                        black_box(run_single_batch_take_reset(
                            batch_size,
                            iters,
                            reset_capacity,
                        ));
                    })
                },
            );
        }

        let mut scratch_capacities = vec![0, 8, 16];
        if !scratch_capacities.contains(&batch_size) {
            scratch_capacities.push(batch_size);
        }
        for initial_capacity in scratch_capacities {
            bench(
                format!(
                    "batch vec single drain-scratch-cap={initial_capacity} batch={batch_size} iters={iters}"
                ),
                move || {
                    elapsed(move || {
                        black_box(run_single_batch_drain_scratch(
                            batch_size,
                            iters,
                            initial_capacity,
                        ));
                    })
                },
            );
        }
    }
}

fn bench_cheap_cpu() {
    for size in SIZES {
        bench(format!("inline cheap-cpu items={size}"), || {
            elapsed(|| {
                black_box(run_inline(size, |item| tiny_work(item as u64) as usize));
            })
        });
        bench(format!("executor cheap-cpu items={size}"), || {
            elapsed(|| {
                run_parallel(WorkOptions::new(), size, |item| {
                    tiny_work(item as u64) as usize
                })
            })
        });
    }
}

fn bench_cpu() {
    for size in SIZES {
        bench(format!("inline cpu items={size}"), || {
            elapsed(|| {
                black_box(run_inline(size, |item| cpu_work(item as u64) as usize));
            })
        });
        bench(format!("executor cpu items={size}"), || {
            elapsed(|| {
                run_parallel(WorkOptions::new(), size, |item| {
                    cpu_work(item as u64) as usize
                })
            })
        });
    }
}

fn bench_io_sleep() {
    for size in [1, 2, 4, 8, 16, 32] {
        bench(format!("inline io-sleep items={size}"), || {
            elapsed(|| {
                black_box(run_inline(size, |item| {
                    thread::sleep(Duration::from_micros(250));
                    item
                }));
            })
        });
        bench(format!("executor io-sleep items={size}"), || {
            elapsed(|| {
                run_parallel(WorkOptions::new().inline_items(1), size, |item| {
                    thread::sleep(Duration::from_micros(250));
                    item
                })
            })
        });
    }
}

fn bench_bytes() {
    for bytes_per_item in [1024, 16 * 1024, 256 * 1024] {
        for size in [1, 2, 4, 8, 16, 32] {
            bench(
                format!("inline bytes item_bytes={bytes_per_item} items={size}"),
                || {
                    elapsed(|| {
                        black_box(run_inline(size, |_| byte_work(bytes_per_item)));
                    })
                },
            );
            bench(
                format!("executor bytes item_bytes={bytes_per_item} items={size}"),
                || {
                    elapsed(move || {
                        let options = WorkOptions::new();
                        let items = (0..size).collect::<Vec<_>>();
                        let input: Items<usize, ()> = Items::ready(items);
                        let results = Work::map(options, input, move |_| byte_work(bytes_per_item));
                        black_box(results);
                    })
                },
            );
        }
    }
}

struct CoarseState {
    inner: Mutex<CoarseInner>,
    changed: Condvar,
}

struct CoarseInner {
    queue: VecDeque<usize>,
    active: usize,
    submitted: usize,
    completed: usize,
}

impl CoarseState {
    fn new(initial: impl IntoIterator<Item = usize>) -> Self {
        let queue = initial.into_iter().collect::<VecDeque<_>>();
        let submitted = queue.len();
        Self {
            inner: Mutex::new(CoarseInner {
                queue,
                active: 0,
                submitted,
                completed: 0,
            }),
            changed: Condvar::new(),
        }
    }

    fn pop(&self) -> Option<usize> {
        let mut inner = self.inner.lock().expect("coarse state poisoned");
        loop {
            if let Some(item) = inner.queue.pop_front() {
                inner.active += 1;
                return Some(item);
            }
            if inner.active == 0 {
                self.changed.notify_all();
                return None;
            }
            inner = self.changed.wait(inner).expect("coarse state poisoned");
        }
    }

    fn complete_fixed(&self) {
        let mut inner = self.inner.lock().expect("coarse state poisoned");
        inner.completed += 1;
        inner.active -= 1;
        self.changed.notify_all();
    }

    fn complete_with_fanout(&self, target: usize) {
        let mut inner = self.inner.lock().expect("coarse state poisoned");
        for _ in 0..2 {
            if inner.submitted >= target {
                break;
            }
            let item = inner.submitted;
            inner.submitted += 1;
            inner.queue.push_back(item);
        }
        inner.completed += 1;
        inner.active -= 1;
        self.changed.notify_all();
    }

    fn completed(&self) -> usize {
        self.inner.lock().expect("coarse state poisoned").completed
    }
}

fn run_coarse_mutex_fixed(workers: usize, items: usize) -> usize {
    let state = Arc::new(CoarseState::new(0..items));
    let handles = (0..workers)
        .map(|_| {
            let state = Arc::clone(&state);
            thread::spawn(move || {
                while let Some(item) = state.pop() {
                    black_box(tiny_work(item as u64));
                    state.complete_fixed();
                }
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().expect("worker panicked");
    }
    state.completed()
}

fn run_coarse_mutex_fanout(workers: usize, items: usize) -> usize {
    let state = Arc::new(CoarseState::new([0]));
    let handles = (0..workers)
        .map(|_| {
            let state = Arc::clone(&state);
            thread::spawn(move || {
                while let Some(item) = state.pop() {
                    black_box(tiny_work(item as u64));
                    state.complete_with_fanout(items);
                }
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().expect("worker panicked");
    }
    state.completed()
}

struct WaitGroup {
    pending: AtomicUsize,
    lock: Mutex<()>,
    cond: Condvar,
}

impl WaitGroup {
    fn new() -> Self {
        Self {
            pending: AtomicUsize::new(0),
            lock: Mutex::new(()),
            cond: Condvar::new(),
        }
    }

    fn submit(&self) {
        self.pending.fetch_add(1, Ordering::SeqCst);
    }

    fn complete(&self) {
        if self.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
            let _lock = self.lock.lock().expect("wait group poisoned");
            self.cond.notify_all();
        }
    }

    fn join(&self) {
        let mut lock = self.lock.lock().expect("wait group poisoned");
        while self.pending.load(Ordering::SeqCst) > 0 {
            lock = self.cond.wait(lock).expect("wait group poisoned");
        }
    }
}

enum WorkMessage {
    Work(usize),
    Stop,
}

fn run_flume_waitgroup_fixed(workers: usize, items: usize) -> usize {
    let (tx, rx) = flume::unbounded();
    let wait = Arc::new(WaitGroup::new());
    let completed = Arc::new(AtomicUsize::new(0));

    let handles = (0..workers)
        .map(|_| {
            let rx = rx.clone();
            let wait = Arc::clone(&wait);
            let completed = Arc::clone(&completed);
            thread::spawn(move || {
                while let Ok(message) = rx.recv() {
                    match message {
                        WorkMessage::Work(item) => {
                            black_box(tiny_work(item as u64));
                            completed.fetch_add(1, Ordering::Relaxed);
                            wait.complete();
                        }
                        WorkMessage::Stop => break,
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    for item in 0..items {
        wait.submit();
        tx.send(WorkMessage::Work(item)).expect("work send failed");
    }
    wait.join();
    for _ in 0..workers {
        tx.send(WorkMessage::Stop).expect("stop send failed");
    }
    for handle in handles {
        handle.join().expect("worker panicked");
    }

    completed.load(Ordering::Relaxed)
}

fn run_flume_waitgroup_fanout(workers: usize, items: usize) -> usize {
    let (tx, rx) = flume::unbounded();
    let wait = Arc::new(WaitGroup::new());
    let submitted = Arc::new(AtomicUsize::new(1));
    let completed = Arc::new(AtomicUsize::new(0));

    let handles = (0..workers)
        .map(|_| {
            let tx = tx.clone();
            let rx = rx.clone();
            let wait = Arc::clone(&wait);
            let submitted = Arc::clone(&submitted);
            let completed = Arc::clone(&completed);
            thread::spawn(move || {
                while let Ok(message) = rx.recv() {
                    match message {
                        WorkMessage::Work(item) => {
                            black_box(tiny_work(item as u64));
                            for _ in 0..2 {
                                let child = submitted.fetch_add(1, Ordering::Relaxed);
                                if child < items {
                                    wait.submit();
                                    tx.send(WorkMessage::Work(child)).expect("work send failed");
                                }
                            }
                            completed.fetch_add(1, Ordering::Relaxed);
                            wait.complete();
                        }
                        WorkMessage::Stop => break,
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    wait.submit();
    tx.send(WorkMessage::Work(0)).expect("work send failed");
    wait.join();
    for _ in 0..workers {
        tx.send(WorkMessage::Stop).expect("stop send failed");
    }
    for handle in handles {
        handle.join().expect("worker panicked");
    }

    completed.load(Ordering::Relaxed)
}

fn run_crossfire_waitgroup_fixed(workers: usize, items: usize) -> usize {
    let (tx, rx) = crossfire::mpmc::unbounded_blocking();
    let wait = Arc::new(WaitGroup::new());
    let completed = Arc::new(AtomicUsize::new(0));

    let handles = (0..workers)
        .map(|_| {
            let rx = rx.clone();
            let wait = Arc::clone(&wait);
            let completed = Arc::clone(&completed);
            thread::spawn(move || {
                while let Ok(message) = rx.recv() {
                    match message {
                        WorkMessage::Work(item) => {
                            black_box(tiny_work(item as u64));
                            completed.fetch_add(1, Ordering::Relaxed);
                            wait.complete();
                        }
                        WorkMessage::Stop => break,
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    for item in 0..items {
        wait.submit();
        tx.send(WorkMessage::Work(item)).expect("work send failed");
    }
    wait.join();
    for _ in 0..workers {
        tx.send(WorkMessage::Stop).expect("stop send failed");
    }
    for handle in handles {
        handle.join().expect("worker panicked");
    }

    completed.load(Ordering::Relaxed)
}

fn run_crossfire_waitgroup_fanout(workers: usize, items: usize) -> usize {
    let (tx, rx) = crossfire::mpmc::unbounded_blocking();
    let wait = Arc::new(WaitGroup::new());
    let submitted = Arc::new(AtomicUsize::new(1));
    let completed = Arc::new(AtomicUsize::new(0));

    let handles = (0..workers)
        .map(|_| {
            let tx = tx.clone();
            let rx = rx.clone();
            let wait = Arc::clone(&wait);
            let submitted = Arc::clone(&submitted);
            let completed = Arc::clone(&completed);
            thread::spawn(move || {
                while let Ok(message) = rx.recv() {
                    match message {
                        WorkMessage::Work(item) => {
                            black_box(tiny_work(item as u64));
                            for _ in 0..2 {
                                let child = submitted.fetch_add(1, Ordering::Relaxed);
                                if child < items {
                                    wait.submit();
                                    tx.send(WorkMessage::Work(child)).expect("work send failed");
                                }
                            }
                            completed.fetch_add(1, Ordering::Relaxed);
                            wait.complete();
                        }
                        WorkMessage::Stop => break,
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    wait.submit();
    tx.send(WorkMessage::Work(0)).expect("work send failed");
    wait.join();
    for _ in 0..workers {
        tx.send(WorkMessage::Stop).expect("stop send failed");
    }
    for handle in handles {
        handle.join().expect("worker panicked");
    }

    completed.load(Ordering::Relaxed)
}

enum CrossfireGuardMessage {
    Work(usize, crossfire::waitgroup::WaitGroupGuard<()>),
    Stop,
}

fn run_crossfire_guard_fixed(workers: usize, items: usize) -> usize {
    let (tx, rx) = crossfire::mpmc::unbounded_blocking();
    let wait = crossfire::waitgroup::WaitGroup::new((), 0);
    let completed = Arc::new(AtomicUsize::new(0));

    let handles = (0..workers)
        .map(|_| {
            let rx = rx.clone();
            let completed = Arc::clone(&completed);
            thread::spawn(move || {
                while let Ok(message) = rx.recv() {
                    match message {
                        CrossfireGuardMessage::Work(item, _guard) => {
                            black_box(tiny_work(item as u64));
                            completed.fetch_add(1, Ordering::Relaxed);
                        }
                        CrossfireGuardMessage::Stop => break,
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    for item in 0..items {
        tx.send(CrossfireGuardMessage::Work(item, wait.add_guard()))
            .expect("work send failed");
    }
    wait.wait();
    for _ in 0..workers {
        tx.send(CrossfireGuardMessage::Stop)
            .expect("stop send failed");
    }
    for handle in handles {
        handle.join().expect("worker panicked");
    }

    completed.load(Ordering::Relaxed)
}

fn run_crossfire_guard_fanout(workers: usize, items: usize) -> usize {
    let (tx, rx) = crossfire::mpmc::unbounded_blocking();
    let wait = crossfire::waitgroup::WaitGroup::new((), 0);
    let submitted = Arc::new(AtomicUsize::new(1));
    let completed = Arc::new(AtomicUsize::new(0));

    let handles = (0..workers)
        .map(|_| {
            let tx = tx.clone();
            let rx = rx.clone();
            let submitted = Arc::clone(&submitted);
            let completed = Arc::clone(&completed);
            thread::spawn(move || {
                while let Ok(message) = rx.recv() {
                    match message {
                        CrossfireGuardMessage::Work(item, guard) => {
                            black_box(tiny_work(item as u64));
                            for _ in 0..2 {
                                let child = submitted.fetch_add(1, Ordering::Relaxed);
                                if child < items {
                                    tx.send(CrossfireGuardMessage::Work(child, guard.clone()))
                                        .expect("work send failed");
                                }
                            }
                            completed.fetch_add(1, Ordering::Relaxed);
                        }
                        CrossfireGuardMessage::Stop => break,
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    tx.send(CrossfireGuardMessage::Work(0, wait.add_guard()))
        .expect("work send failed");
    wait.wait();
    for _ in 0..workers {
        tx.send(CrossfireGuardMessage::Stop)
            .expect("stop send failed");
    }
    for handle in handles {
        handle.join().expect("worker panicked");
    }

    completed.load(Ordering::Relaxed)
}

fn bench_workset_coordination() {
    for workers in WORKSET_WORKERS {
        for items in WORKSET_ITEMS {
            bench(
                format!("workset coarse-mutex fixed workers={workers} items={items}"),
                || {
                    elapsed(|| {
                        black_box(run_coarse_mutex_fixed(workers, items));
                    })
                },
            );
            bench(
                format!("workset flume-waitgroup fixed workers={workers} items={items}"),
                || {
                    elapsed(|| {
                        black_box(run_flume_waitgroup_fixed(workers, items));
                    })
                },
            );
            bench(
                format!("workset crossfire-waitgroup fixed workers={workers} items={items}"),
                || {
                    elapsed(|| {
                        black_box(run_crossfire_waitgroup_fixed(workers, items));
                    })
                },
            );
            bench(
                format!("workset crossfire-guard fixed workers={workers} items={items}"),
                || {
                    elapsed(|| {
                        black_box(run_crossfire_guard_fixed(workers, items));
                    })
                },
            );
            bench(
                format!("workset coarse-mutex fanout workers={workers} items={items}"),
                || {
                    elapsed(|| {
                        black_box(run_coarse_mutex_fanout(workers, items));
                    })
                },
            );
            bench(
                format!("workset flume-waitgroup fanout workers={workers} items={items}"),
                || {
                    elapsed(|| {
                        black_box(run_flume_waitgroup_fanout(workers, items));
                    })
                },
            );
            bench(
                format!("workset crossfire-waitgroup fanout workers={workers} items={items}"),
                || {
                    elapsed(|| {
                        black_box(run_crossfire_waitgroup_fanout(workers, items));
                    })
                },
            );
            bench(
                format!("workset crossfire-guard fanout workers={workers} items={items}"),
                || {
                    elapsed(|| {
                        black_box(run_crossfire_guard_fanout(workers, items));
                    })
                },
            );
        }
    }
}

fn run_workset_small_batches(workers: usize, batches: usize) -> usize {
    let input = (0..batches)
        .map(|batch| (batch * 16..batch * 16 + 16).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let completed = Arc::new(AtomicUsize::new(0));
    let completed_worker = Arc::clone(&completed);

    let items = Work::run(
        WorkOptions::new()
            .max_workers(workers)
            .inline_items(0)
            .work_chunk_items(16)
            .result_batch_items(16),
        Items::stream(input.into_iter().map(Ok)),
        WorkShape::batch(
            move |batch: Vec<usize>, scope: &mut WorkScope<'_, usize, usize, ()>| {
                completed_worker.fetch_add(batch.len(), Ordering::Relaxed);
                scope.send_result(batch.into_iter().inspect(|&item| {
                    black_box(tiny_work(item as u64));
                }));
                Ok(())
            },
        ),
    )
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .unwrap();

    completed.load(Ordering::Relaxed) ^ items.len()
}

fn bench_workset_small_batch_spawn() {
    for workers in WORKSET_SPAWN_WORKERS {
        for batches in WORKSET_SPAWN_BATCHES {
            bench(
                format!("workset small-batch-spawn workers={workers} batches={batches}"),
                || {
                    elapsed(|| {
                        black_box(run_workset_small_batches(workers, batches));
                    })
                },
            );
        }
    }
}

fn run_workset_fanout(target: usize, inline_items: usize, expensive: bool) -> usize {
    let submitted = Arc::new(AtomicUsize::new(1));
    let completed = Arc::new(AtomicUsize::new(0));
    let submitted_worker = Arc::clone(&submitted);
    let completed_worker = Arc::clone(&completed);

    Work::run(
        WorkOptions::new().max_workers(8).inline_items(inline_items),
        Items::ready(vec![0usize]),
        WorkShape::batch(
            move |batch: Vec<usize>, scope: &mut WorkScope<'_, usize, (), ()>| {
                for item in batch {
                    if expensive {
                        black_box(cpu_work(item as u64));
                    } else {
                        black_box(tiny_work(item as u64));
                    }
                    completed_worker.fetch_add(1, Ordering::Relaxed);
                    for _ in 0..2 {
                        let child = submitted_worker.fetch_add(1, Ordering::Relaxed);
                        if child < target {
                            scope.submit_work(child);
                        }
                    }
                }
                Ok(())
            },
        ),
    )
    .drain_until_error()
    .unwrap();

    completed.load(Ordering::Relaxed)
}

fn bench_workset_transition() {
    for target in WORKSET_TRANSITION_ITEMS {
        bench(format!("workset inline tiny-fanout items={target}"), || {
            elapsed(|| {
                black_box(run_workset_fanout(target, usize::MAX, false));
            })
        });
        bench(
            format!("workset default-threshold tiny-fanout items={target}"),
            || {
                elapsed(|| {
                    black_box(run_workset_fanout(target, 16, false));
                })
            },
        );
        bench(
            format!("workset forced-promote tiny-fanout items={target}"),
            || {
                elapsed(|| {
                    black_box(run_workset_fanout(target, 0, false));
                })
            },
        );
    }

    for target in WORKSET_TRANSITION_ITEMS {
        bench(format!("workset inline cpu-fanout items={target}"), || {
            elapsed(|| {
                black_box(run_workset_fanout(target, usize::MAX, true));
            })
        });
        bench(
            format!("workset default-threshold cpu-fanout items={target}"),
            || {
                elapsed(|| {
                    black_box(run_workset_fanout(target, 16, true));
                })
            },
        );
        bench(
            format!("workset forced-promote cpu-fanout items={target}"),
            || {
                elapsed(|| {
                    black_box(run_workset_fanout(target, 0, true));
                })
            },
        );
    }
}

fn main() {
    crossfire::detect_backoff_cfg();
    bench_channel_allocation();
    bench_queue_throughput();
    bench_batch_buffers();
    bench_batch_buffers_single();
    bench_cheap_cpu();
    bench_cpu();
    bench_io_sleep();
    bench_bytes();
    bench_workset_coordination();
    bench_workset_small_batch_spawn();
    bench_workset_transition();
}
