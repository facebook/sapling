/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blackbox::{
    event::{Event, NetworkOp},
    Blackbox, BlackboxOptions, IndexFilter,
};
use minibench::{
    bench,
    measure::{Both, Bytes, Measure, WallClock, IO},
};
use serde_json::json;
use tempfile::tempdir;

// Insert 4000 entries
fn insert(blackbox: &mut Blackbox) {
    for _ in 0..2000 {
        blackbox.log(&Event::Alias {
            from: "foo".to_string(),
            to: "bar".to_string(),
        });
        blackbox.log(&Event::Network {
            op: NetworkOp::SshGetFiles,
            read_bytes: 1000,
            write_bytes: 9000,
            duration_ms: 100,
            calls: 2,
            latency_ms: 10,
            result: None,
        });
    }
    blackbox.sync();
}

fn main() {
    bench("blackbox insertion (4000 entries)", || {
        let dir = tempdir().unwrap();
        let mut blackbox = BlackboxOptions::new().open(dir.path()).unwrap();
        Both::<Both<WallClock, IO>, Bytes>::measure(move || {
            insert(&mut blackbox);
            dir.path().join("0").join("log").metadata().unwrap().len()
        })
    });

    {
        let dir = tempdir().unwrap();
        let mut blackbox = BlackboxOptions::new().open(dir.path()).unwrap();
        insert(&mut blackbox);

        bench("blackbox filter by index (4000 entries)", || {
            Both::<WallClock, IO>::measure(|| {
                blackbox.filter::<Event>(IndexFilter::Time(0, u64::max_value()), None);
            })
        });

        bench("blackbox filter by pattern (4000 entries)", || {
            Both::<WallClock, IO>::measure(|| {
                blackbox.filter::<Event>(
                    IndexFilter::Nop,
                    Some(json!({"network": {"read_bytes": ["range", 10, 2000]}})),
                );
            })
        });
    }
}
