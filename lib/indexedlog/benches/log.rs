// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use indexedlog::log::{IndexDef, IndexOutput, Log};
use minibench::{bench, elapsed, measure, Measure};
use rand::{ChaChaRng, Rng};
use std::path::Path;
use tempfile::tempdir;

const N: usize = 204800;

/// Generate random buffer
fn gen_buf(size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size];
    ChaChaRng::new_unseeded().fill_bytes(buf.as_mut());
    buf
}

/// Get a `Log` with index defined.
fn log_with_index(path: &Path, lag: u64) -> Log {
    let index_func = |_data: &[u8]| vec![IndexOutput::Reference(0..20)];
    let index_def = IndexDef::new("i", index_func).lag_threshold(lag);
    Log::open(path, vec![index_def]).unwrap()
}

/// Measure both elapsed and IO.
type MeasureSync = measure::Both<measure::WallClock, measure::IO>;

fn main() {
    bench("log insertion", || {
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), vec![]).unwrap();
        let buf = gen_buf(N * 20);
        elapsed(move || {
            for i in 0..N {
                log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
            }
        })
    });

    bench("log insertion with index", || {
        let dir = tempdir().unwrap();
        let mut log = log_with_index(dir.path(), 0);
        let buf = gen_buf(N * 20);
        elapsed(move || {
            for i in 0..N {
                log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
            }
        })
    });

    bench("log sync (write N, no index)", || {
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), vec![]).unwrap();
        let buf = gen_buf(N * 20);
        for i in 0..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        MeasureSync::measure(|| {
            log.sync().unwrap();
        })
    });

    bench("log sync (write N)", || {
        let dir = tempdir().unwrap();
        let buf = gen_buf(N * 20);
        let mut log = log_with_index(dir.path(), 0);
        // Write one entry to make things more interesting.
        log.append(&buf[0..20]).unwrap();
        log.sync().unwrap();
        for i in 1..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        MeasureSync::measure(|| {
            log.sync().unwrap();
        })
    });

    bench("log sync (write 1, update index)", || {
        let dir = tempdir().unwrap();
        let buf = gen_buf(N * 20);
        let mut log = log_with_index(dir.path(), 0);
        log.append(&buf[0..20]).unwrap();
        log.sync().unwrap();
        log.append(&buf[20..40]).unwrap();
        MeasureSync::measure(|| {
            log.sync().unwrap();
        })
    });

    bench("log sync (write 1, not update index)", || {
        let dir = tempdir().unwrap();
        let buf = gen_buf(N * 20);
        let mut log = log_with_index(dir.path(), u64::max_value());
        log.append(&buf[0..20]).unwrap();
        log.sync().unwrap();
        log.append(&buf[20..40]).unwrap();
        MeasureSync::measure(|| {
            log.sync().unwrap();
        })
    });

    bench("log sync (read, no-op)", || {
        let dir = tempdir().unwrap();
        let buf = gen_buf(N * 20);
        let mut log = log_with_index(dir.path(), 0);
        for i in 0..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        log.sync().unwrap();
        let mut log = log_with_index(dir.path(), 0);
        MeasureSync::measure(|| {
            log.sync().unwrap();
        })
    });

    bench("log sync (read, new log 0, index lag N)", || {
        let dir = tempdir().unwrap();
        let buf = gen_buf(N * 20);
        let mut log = log_with_index(dir.path(), u64::max_value());
        for i in 0..(N / 2) {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        log.sync().unwrap();
        MeasureSync::measure(|| {
            log.sync().unwrap();
        })
    });

    bench("log sync (read, new log 1, index lag N)", || {
        let dir = tempdir().unwrap();
        let buf = gen_buf(N * 20);
        let mut log = log_with_index(dir.path(), u64::max_value());
        for i in 1..(N / 2) {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        log.sync().unwrap();
        let mut log2 = log_with_index(dir.path(), u64::max_value());
        log2.append(&buf[0..20]).unwrap();
        log2.sync().unwrap();
        MeasureSync::measure(|| {
            log.sync().unwrap();
        })
    });

    bench("log sync (read, new log N, index lag 0)", || {
        let dir = tempdir().unwrap();
        let buf = gen_buf(N * 20);
        let mut log = log_with_index(dir.path(), 0);
        let mut log2 = log_with_index(dir.path(), 0);
        for i in 0..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        log.sync().unwrap();
        MeasureSync::measure(|| {
            log2.sync().unwrap();
        })
    });

    bench("log sync (read, new log N, index lag N)", || {
        let dir = tempdir().unwrap();
        let buf = gen_buf(N * 20);
        let mut log = log_with_index(dir.path(), u64::max_value());
        let mut log2 = log_with_index(dir.path(), u64::max_value());
        for i in 0..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        log.sync().unwrap();
        MeasureSync::measure(|| {
            log2.sync().unwrap();
        })
    });

    bench("log sync (read 1, write N)", || {
        let dir = tempdir().unwrap();
        let buf = gen_buf(N * 20);
        let mut log = log_with_index(dir.path(), 0);
        let mut log2 = log_with_index(dir.path(), 0);
        log.append(&buf[0..20]).unwrap();
        log.sync().unwrap();
        log2.append(&buf[20..40]).unwrap();
        for i in 2..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        log2.sync().unwrap();
        MeasureSync::measure(|| {
            log.sync().unwrap();
        })
    });

    bench("log sync (read N, write 1)", || {
        let dir = tempdir().unwrap();
        let buf = gen_buf(N * 20);
        let mut log = log_with_index(dir.path(), 0);
        let mut log2 = log_with_index(dir.path(), 0);
        log.append(&buf[0..20]).unwrap();
        log.sync().unwrap();
        log2.append(&buf[20..40]).unwrap();
        for i in 2..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        log.sync().unwrap();
        MeasureSync::measure(|| {
            log2.sync().unwrap();
        })
    });

    bench("log iteration (memory)", || {
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), vec![]).unwrap();
        let buf = gen_buf(N * 20);
        for i in 0..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        elapsed(move || {
            log.iter().count();
        })
    });

    bench("log iteration (disk)", || {
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), vec![]).unwrap();
        let buf = gen_buf(N * 20);
        for i in 0..N {
            log.append(&buf[20 * i..20 * (i + 1)]).unwrap();
        }
        log.sync().unwrap();
        let log = Log::open(dir.path(), vec![]).unwrap();
        elapsed(move || {
            log.iter().count();
        })
    });
}
