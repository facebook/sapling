/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cell::RefCell;
#[cfg(not(windows))]
use std::io::Read;
#[cfg(not(windows))]
use std::ops::Range;
use std::sync::mpsc::channel;
use std::time::Duration;

use quickcheck::quickcheck;
use tempfile::tempdir;

use super::*;

#[derive(Debug)]
struct DummyError(&'static str);

impl fmt::Display for DummyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DummyError {}

#[test]
fn test_empty_log() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("log");
    let log1 = Log::open(&log_path, Vec::new()).unwrap();
    assert_eq!(log1.iter().count(), 0);
    let log2 = Log::open(&log_path, Vec::new()).unwrap();
    assert_eq!(log2.iter().count(), 0);
}

#[test]
fn test_open_options_create() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("log1");

    let opts = OpenOptions::new();
    assert!(opts.open(&log_path).is_err());

    let opts = OpenOptions::new().create(true);
    assert!(opts.open(&log_path).is_ok());

    let opts = OpenOptions::new().create(false);
    assert!(opts.open(&log_path).is_ok());

    let log_path = dir.path().join("log2");
    let opts = OpenOptions::new().create(false);
    assert!(opts.open(log_path).is_err());
}

#[test]
fn test_incomplete_rewrite() {
    let dir = tempdir().unwrap();
    let read_entries = || -> Vec<Vec<u8>> {
        let log = Log::open(&dir, Vec::new()).unwrap();
        log.iter()
            .map(|v| v.map(|v| v.to_vec()))
            .collect::<Result<Vec<Vec<u8>>, _>>()
            .unwrap()
    };
    let add_noise = |noise: &[u8]| {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("log"))
            .unwrap();
        // Emulate an incomplete write with broken data.
        file.write_all(noise).unwrap();
    };

    let mut log1 = Log::open(&dir, Vec::new()).unwrap();
    log1.append(b"abc").unwrap();
    log1.sync().unwrap();
    assert_eq!(read_entries(), vec![b"abc"]);

    add_noise(&[0xcc; 1]);
    assert_eq!(read_entries(), vec![b"abc"]);

    log1.append(b"def").unwrap();
    log1.sync().unwrap();
    assert_eq!(read_entries(), vec![b"abc", b"def"]);

    add_noise(&[0xcc; 1000]);
    assert_eq!(read_entries(), vec![b"abc", b"def"]);

    log1.append(b"ghi").unwrap();
    log1.sync().unwrap();
    assert_eq!(read_entries(), vec![b"abc", b"def", b"ghi"]);

    add_noise(&[0xcc; 1000]);
    assert_eq!(read_entries(), vec![b"abc", b"def", b"ghi"]);
}

#[test]
fn test_checksum_type() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("log");

    let open = |checksum_type| {
        OpenOptions::new()
            .checksum_type(checksum_type)
            .create(true)
            .open(&log_path)
            .unwrap()
    };

    let short_bytes = vec![12; 20];
    let long_bytes = vec![24; 200];
    let mut expected = Vec::new();

    let mut log = open(ChecksumType::Auto);
    log.append(&short_bytes).unwrap();
    expected.push(short_bytes.clone());
    log.append(&long_bytes).unwrap();
    expected.push(long_bytes.clone());
    log.sync().unwrap();

    let mut log = open(ChecksumType::Xxhash32);
    log.append(&long_bytes).unwrap();
    expected.push(long_bytes.clone());
    log.sync().unwrap();

    let mut log = open(ChecksumType::Xxhash64);
    log.append(&short_bytes).unwrap();
    expected.push(short_bytes.clone());

    assert_eq!(
        log.iter()
            .map(|v| v.unwrap().to_vec())
            .collect::<Vec<Vec<u8>>>(),
        expected,
    );

    // Reload and verify
    assert_eq!(log.sync().unwrap(), 486);

    let log = Log::open(&log_path, Vec::new()).unwrap();
    assert_eq!(
        log.iter()
            .map(|v| v.unwrap().to_vec())
            .collect::<Vec<Vec<u8>>>(),
        expected,
    );
}

#[test]
fn test_iter_and_iter_dirty() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("log");
    let mut log = Log::open(log_path, Vec::new()).unwrap();

    log.append(b"2").unwrap();
    log.append(b"4").unwrap();
    log.append(b"3").unwrap();

    assert_eq!(
        log.iter().collect::<crate::Result<Vec<_>>>().unwrap(),
        vec![b"2", b"4", b"3"]
    );
    assert_eq!(
        log.iter().collect::<crate::Result<Vec<_>>>().unwrap(),
        log.iter_dirty().collect::<crate::Result<Vec<_>>>().unwrap(),
    );

    log.sync().unwrap();

    assert!(
        log.iter_dirty()
            .collect::<crate::Result<Vec<_>>>()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        log.iter().collect::<crate::Result<Vec<_>>>().unwrap(),
        vec![b"2", b"4", b"3"]
    );

    log.append(b"5").unwrap();
    log.append(b"1").unwrap();
    assert_eq!(
        log.iter_dirty().collect::<crate::Result<Vec<_>>>().unwrap(),
        vec![b"5", b"1"]
    );
    assert_eq!(
        log.iter().collect::<crate::Result<Vec<_>>>().unwrap(),
        vec![b"2", b"4", b"3", b"5", b"1"]
    );
}

fn get_index_defs(lag_threshold: u64) -> Vec<IndexDef> {
    // Two index functions. First takes every 2 bytes as references. The second takes every 3
    // bytes as owned slices.
    // Keys starting with '-' are considered as "deletion" requests.
    // Keys starting with '=' are considered as "delete prefix" requests.
    let index_func0 = |data: &[u8]| {
        if data.first() == Some(&b'=') {
            return vec![IndexOutput::RemovePrefix(
                data[1..].to_vec().into_boxed_slice(),
            )];
        }
        let is_removal = data.first() == Some(&b'-');
        let start = if is_removal { 1 } else { 0 };
        (start..(data.len().max(1) - 1))
            .map(|i| {
                if is_removal {
                    IndexOutput::Remove(data[i..i + 2].to_vec().into_boxed_slice())
                } else {
                    IndexOutput::Reference(i as u64..i as u64 + 2)
                }
            })
            .collect()
    };
    let index_func1 = |data: &[u8]| {
        if data.first() == Some(&b'=') {
            return vec![IndexOutput::RemovePrefix(
                data[1..].to_vec().into_boxed_slice(),
            )];
        }
        let is_removal = data.first() == Some(&b'-');
        let start = if is_removal { 1 } else { 0 };
        (start..(data.len().max(2) - 2))
            .map(|i| {
                let bytes = data[i..i + 3].to_vec().into_boxed_slice();
                if is_removal {
                    IndexOutput::Remove(bytes)
                } else {
                    IndexOutput::Owned(bytes)
                }
            })
            .collect()
    };
    vec![
        IndexDef::new("x", index_func0).lag_threshold(lag_threshold),
        IndexDef::new("y", index_func1).lag_threshold(lag_threshold),
    ]
}

#[test]
fn test_slice_to_bytes() {
    let dir = tempdir().unwrap();
    let mut log = Log::open(dir.path(), get_index_defs(0)).unwrap();
    log.append(b"0123456").unwrap();
    log.append(b"1231516").unwrap();
    log.sync().unwrap();

    let slice = log.lookup(0, b"23").unwrap().into_vec().unwrap()[0];
    assert_eq!(slice, b"1231516");

    // The bytes are zero-copy from the Log buffer.
    let bytes1 = log.slice_to_bytes(slice);
    let bytes2 = log.slice_to_bytes(slice);
    assert_eq!(bytes1.as_ptr(), bytes2.as_ptr());

    // No zero-copy from the Index buffer.
    let bytes1 = log.index_slice_to_bytes(0, slice);
    let bytes2 = log.index_slice_to_bytes(0, slice);
    assert_ne!(bytes1.as_ptr(), bytes2.as_ptr());

    // Try IndexOutput::Reference produced by Index #0.
    let key = log.lookup_range(0, ..).unwrap().next().unwrap().unwrap().0;
    assert_eq!(&key[..], b"01");

    // The key is from the main Log buffer.
    let bytes1 = log.slice_to_bytes(&key);
    let bytes2 = log.slice_to_bytes(&key);
    assert_eq!(bytes1.as_ptr(), bytes2.as_ptr());
    let bytes1 = log.index_slice_to_bytes(0, &key);
    let bytes2 = log.index_slice_to_bytes(0, &key);
    assert_ne!(bytes1.as_ptr(), bytes2.as_ptr());

    // Try IndexOutput::Owned produced by Index #1.
    let key = log.lookup_range(1, ..).unwrap().next().unwrap().unwrap().0;
    assert_eq!(&key[..], b"012");

    // The key is from the Index buffer.
    let bytes1 = log.slice_to_bytes(&key);
    let bytes2 = log.slice_to_bytes(&key);
    assert_ne!(bytes1.as_ptr(), bytes2.as_ptr());
    let bytes1 = log.index_slice_to_bytes(1, &key);
    let bytes2 = log.index_slice_to_bytes(1, &key);
    assert_eq!(bytes1.as_ptr(), bytes2.as_ptr());
}

#[test]
fn test_fmt_debug() -> crate::Result<()> {
    let dir = tempdir().unwrap();
    let mut log = OpenOptions::new().create(true).open(dir.path())?;
    let entries = [
        "Indexed Log provides an integrity-checked, append-only storage with index support.",
        "Indexed Log 提供一种支持索引和完整性检查的仅追加存储。",
    ];
    for entry in &entries {
        log.append(entry)?;
    }
    assert_eq!(
        format!("\n{:?}", log),
        r#"
# Entry 1:
0000000c: 49 6e 64 65 78 65 64 20 4c 6f 67 20 70 72 6f 76  Indexed Log prov
0000001c: 69 64 65 73 20 61 6e 20 69 6e 74 65 67 72 69 74  ides an integrit
0000002c: 79 2d 63 68 65 63 6b 65 64 2c 20 61 70 70 65 6e  y-checked, appen
0000003c: 64 2d 6f 6e 6c 79 20 73 74 6f 72 61 67 65 20 77  d-only storage w
0000004c: 69 74 68 20 69 6e 64 65 78 20 73 75 70 70 6f 72  ith index suppor
0000005c: 74 2e                                            t.

# Entry 2:
00000064: 49 6e 64 65 78 65 64 20 4c 6f 67 20 e6 8f 90 e4  Indexed Log ....
00000074: be 9b e4 b8 80 e7 a7 8d e6 94 af e6 8c 81 e7 b4  ................
00000084: a2 e5 bc 95 e5 92 8c e5 ae 8c e6 95 b4 e6 80 a7  ................
00000094: e6 a3 80 e6 9f a5 e7 9a 84 e4 bb 85 e8 bf bd e5  ................
000000a4: 8a a0 e5 ad 98 e5 82 a8 e3 80 82                 ...........
"#
    );
    Ok(())
}

#[test]
fn test_index_manual() {
    // Test index lookups with these combinations:
    // - Index key: Reference and Owned.
    // - Index lag_threshold: 0, 10, 20, ....
    // - Entries: Mixed on-disk and in-memory ones.
    for lag in [0u64, 10, 20, 50, 1000] {
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), get_index_defs(lag)).unwrap();
        let entries: [&[u8]; 7] = [b"1", b"", b"2345", b"", b"78", b"3456", b"35"];
        for bytes in entries.iter() {
            log.append(bytes).expect("append");
            // Flush and reload in the middle of entries. This exercises the code paths
            // handling both on-disk and in-memory parts.
            if bytes.is_empty() {
                log.sync().expect("flush");
                log = Log::open(dir.path(), get_index_defs(lag)).unwrap();
            }
        }

        // Lookups via index 0
        assert_eq!(
            log.lookup(0, b"34").unwrap().into_vec().unwrap(),
            [b"3456", b"2345"]
        );
        assert_eq!(log.lookup(0, b"56").unwrap().into_vec().unwrap(), [b"3456"]);
        assert_eq!(log.lookup(0, b"78").unwrap().into_vec().unwrap(), [b"78"]);
        assert!(log.lookup(0, b"89").unwrap().into_vec().unwrap().is_empty());

        // Lookups via index 1
        assert_eq!(
            log.lookup(1, b"345").unwrap().into_vec().unwrap(),
            [b"3456", b"2345"]
        );

        log.sync().unwrap();

        // Delete prefix.
        log.append(b"=3").unwrap();
        for key in [b"34", b"35"] {
            assert!(log.lookup(0, key).unwrap().into_vec().unwrap().is_empty());
        }
        assert_eq!(log.lookup(0, b"56").unwrap().into_vec().unwrap(), [b"3456"]);

        // Delete keys.
        let mut log = Log::open(dir.path(), get_index_defs(lag)).unwrap();
        for bytes in entries.iter() {
            let mut bytes = bytes.to_vec();
            bytes.insert(0, b'-');
            log.append(&bytes).unwrap();
            if bytes.is_empty() {
                log.sync().expect("flush");
                log = Log::open(dir.path(), get_index_defs(lag)).unwrap();
            }
        }
        for key in [b"34", b"56", b"78"] {
            assert!(log.lookup(0, key).unwrap().into_vec().unwrap().is_empty());
        }
        assert_eq!(log.lookup(1, b"345").unwrap().count(), 0);
    }
}

#[test]
fn test_index_reorder() {
    let dir = tempdir().unwrap();
    let indexes = get_index_defs(0);
    let mut log = Log::open(dir.path(), indexes).unwrap();
    let entries: [&[u8]; 2] = [b"123", b"234"];
    for bytes in entries.iter() {
        log.append(bytes).expect("append");
    }
    log.sync().expect("flush");
    // Reverse the index to make it interesting.
    let mut indexes = get_index_defs(0);
    indexes.reverse();
    log = Log::open(dir.path(), indexes).unwrap();
    assert_eq!(
        log.lookup(1, b"23").unwrap().into_vec().unwrap(),
        [b"234", b"123"]
    );
}

// This test rewrites mapped files which is unsupoorted by Windows.
#[cfg(not(windows))]
#[test]
fn test_index_mark_corrupt() {
    let dir = tempdir().unwrap();
    let indexes = get_index_defs(0);

    let mut log = Log::open(dir.path(), indexes).unwrap();
    let entries: [&[u8]; 2] = [b"123", b"234"];
    for bytes in entries.iter() {
        log.append(bytes).expect("append");
    }
    log.sync().expect("flush");

    // Corrupt an index. Backup its content.
    let backup = {
        let mut buf = Vec::new();
        let size = File::open(dir.path().join("index2-x"))
            .unwrap()
            .read_to_end(&mut buf)
            .unwrap();
        let mut index_file = File::create(dir.path().join("index2-x")).unwrap();
        index_file.write_all(&vec![0; size]).expect("write");
        buf
    };

    // Inserting a new entry will mark the index as "corrupted".
    assert!(log.append(b"new").is_err());

    // Then index lookups will return errors. Even if its content is restored.
    let mut index_file = File::create(dir.path().join("index2-x")).unwrap();
    index_file.write_all(&backup).expect("write");
    assert!(log.lookup(1, b"23").is_err());
}

#[test]
fn test_lookup_prefix_and_range() {
    let dir = tempdir().unwrap();
    let index_func = |data: &[u8]| vec![IndexOutput::Reference(0..(data.len() - 1) as u64)];
    let mut log = Log::open(
        dir.path(),
        vec![IndexDef::new("simple", index_func).lag_threshold(0)],
    )
    .unwrap();

    let entries = [&b"aaa"[..], b"bb", b"bb"];

    for entry in entries.iter() {
        log.append(entry).unwrap();
    }

    // Test lookup_prefix

    // 0x61 == b'a'. 0x6 will match both keys: "aa" and "b".
    // "aa" matches the value "aaa", "b" matches the entries ["bb", "bb"]
    let mut iter = log.lookup_prefix_hex(0, b"6").unwrap().rev();
    assert_eq!(
        iter.next()
            .unwrap()
            .unwrap()
            .1
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        vec![b"bb", b"bb"]
    );
    assert_eq!(iter.next().unwrap().unwrap().0.as_ref(), b"aa");
    assert!(iter.next().is_none());

    let mut iter = log.lookup_prefix(0, b"b").unwrap();
    assert_eq!(iter.next().unwrap().unwrap().0.as_ref(), b"b");
    assert!(iter.next().is_none());

    // Test lookup_range
    assert_eq!(log.lookup_range(0, &b"b"[..]..).unwrap().count(), 1);
    assert_eq!(log.lookup_range(0, ..=&b"b"[..]).unwrap().count(), 2);
    assert_eq!(
        log.lookup_range(0, &b"c"[..]..=&b"d"[..]).unwrap().count(),
        0
    );

    let mut iter = log.lookup_range(0, ..).unwrap().rev();
    let next = iter.next().unwrap().unwrap();
    assert_eq!(next.0.as_ref(), &b"b"[..]);
    assert_eq!(
        next.1.collect::<Result<Vec<_>, _>>().unwrap(),
        vec![&b"bb"[..], &b"bb"[..]]
    );
    let next = iter.next().unwrap().unwrap();
    assert_eq!(next.0.as_ref(), &b"aa"[..]);
    assert_eq!(
        next.1.collect::<Result<Vec<_>, _>>().unwrap(),
        vec![&b"aaa"[..]]
    );
    assert!(iter.next().is_none());
}

#[test]
fn test_index_func() {
    let dir = tempdir().unwrap();
    let entries = vec![
        b"abcdefghij",
        b"klmnopqrst",
        b"uvwxyz1234",
        b"5678901234",
        b"5678901234",
    ];

    let first_index =
        |_data: &[u8]| vec![IndexOutput::Reference(0..2), IndexOutput::Reference(3..5)];
    let second_index = |data: &[u8]| vec![IndexOutput::Owned(Box::from(&data[5..10]))];
    let third_index = |_: &[u8]| vec![IndexOutput::Owned(Box::from(&b"x"[..]))];
    let mut log = OpenOptions::new()
        .create(true)
        .index_defs(vec![
            IndexDef::new("first", first_index).lag_threshold(0),
            IndexDef::new("second", second_index).lag_threshold(0),
        ])
        .index("third", third_index)
        .open(dir.path())
        .unwrap();

    let mut expected_keys1 = vec![];
    let mut expected_keys2 = vec![];
    for &data in entries {
        log.append(data).expect("append");
        expected_keys1.push(data[0..2].to_vec());
        expected_keys1.push(data[3..5].to_vec());
        expected_keys2.push(data[5..10].to_vec());
    }

    let mut found_keys1 = vec![];
    let mut found_keys2 = vec![];

    for entry in log.iter() {
        let entry = entry.unwrap();
        found_keys1.extend(
            log.index_func(0, entry)
                .unwrap()
                .into_iter()
                .map(|c| c.into_owned()),
        );
        found_keys2.extend(
            log.index_func(1, entry)
                .unwrap()
                .into_iter()
                .map(|c| c.into_owned()),
        );
    }
    assert_eq!(found_keys1, expected_keys1);
    assert_eq!(found_keys2, expected_keys2);
    assert_eq!(log.iter().count(), log.lookup(2, b"x").unwrap().count());
}

#[test]
fn test_index_lag_threshold() {
    for lag_entries in &[0, 1] {
        //, 2, 5, 8] {
        let lag_bytes = lag_entries * 110; // 100: entry; 8: xxhash; 1: vlq len; 1: flag
        let dir = tempdir().unwrap();
        let def =
            IndexDef::new("a", |_| vec![IndexOutput::Reference(0..1)]).lag_threshold(lag_bytes);
        let index_filename = def.filename();
        let open_opts = OpenOptions::new().create(true).index_defs(vec![def]);
        let get_index_size = || -> u64 {
            let index_path = dir.path().join(&index_filename);
            index_path.metadata().map(|m| m.len()).unwrap_or(0)
        };
        let mut unindexed_entries = 0;
        let mut log = open_opts.open(dir.path()).unwrap();
        for i in 0..10 {
            let index_size_before = get_index_size();
            if i % 3 == 0 {
                // This is optional, but makes the test more interesting.
                log = open_opts.open(dir.path()).unwrap();
            }
            log.append(vec![i as u8; 100]).unwrap();
            log.sync().unwrap();
            let index_size_after = get_index_size();
            let index_should_change = unindexed_entries >= *lag_entries;
            if index_should_change {
                unindexed_entries = 0;
            } else {
                unindexed_entries += 1;
            }
            let index_changed = index_size_after != index_size_before;
            assert_eq!(index_should_change, index_changed);
        }
    }
}

#[test]
fn test_update_index_upon_open() {
    // Indexes that are lagging are updated on open.
    let dir = tempdir().unwrap();
    let entry_size = 9; // 3 chars + 4 xxhash + 1 vlq length + 1 flag
    let def =
        IndexDef::new("a", |_| vec![IndexOutput::Reference(0..1)]).lag_threshold(3 * entry_size);
    let index_filename = def.filename();
    let open_opts = OpenOptions::new().create(true).index_defs(vec![def]);
    let get_index_size = || -> u64 {
        let index_path = dir.path().join(&index_filename);
        index_path.metadata().map(|m| m.len()).unwrap_or(0)
    };
    assert_eq!(get_index_size(), 0);
    let mut log = open_opts.open(dir.path()).unwrap();
    log.append(b"abc").unwrap();
    log.append(b"abc").unwrap();
    log.append(b"abc").unwrap();
    log.sync().unwrap();
    assert_eq!(
        get_index_size(),
        0,
        "index should be empty as 3 entries are within lag threshold 3"
    );

    log.append(b"abc").unwrap();
    log.sync().unwrap();
    assert_eq!(
        get_index_size(),
        47,
        "index should not be empty as 4 entries exceed lag threshold 3"
    );

    log.append(b"abc").unwrap();
    log.append(b"abc").unwrap();
    log.sync().unwrap();
    assert_eq!(
        get_index_size(),
        47,
        "index should not be changed because 2 new entries are within lag threshold 3"
    );

    // Open with a different OpenOptions. This should update the indexes.
    let def = IndexDef::new("a", |_| vec![IndexOutput::Reference(0..1)]).lag_threshold(entry_size);
    let open_opts = OpenOptions::new().create(true).index_defs(vec![def]);
    let _log = open_opts.open(dir.path()).unwrap();
    assert_eq!(
        get_index_size(),
        84,
        "index should be changed because 2 pending entries exceeds lag threshold 1"
    );

    // Open with IndexDef renamed.
    let def =
        IndexDef::new("b", |_| vec![IndexOutput::Reference(0..1)]).lag_threshold(4 * entry_size);
    let index_filename = def.filename();
    let open_opts = OpenOptions::new().create(true).index_defs(vec![def]);
    let _log = open_opts.open(dir.path()).unwrap();
    assert_eq!(
        dir.path().join(index_filename).metadata().unwrap().len(),
        53,
        "new index should be built at open time since 6 entries exceeds threshold 4"
    );

    let def =
        IndexDef::new("c", |_| vec![IndexOutput::Reference(0..1)]).lag_threshold(6 * entry_size);
    let index_filename = def.filename();
    let open_opts = OpenOptions::new().create(true).index_defs(vec![def]);
    let _log = open_opts.open(dir.path()).unwrap();
    assert_eq!(
        dir.path().join(index_filename).metadata().unwrap().len(),
        0,
        "new index should be built at open time since 6 entries do not exceed threshold 6"
    );
}

#[test]
fn test_flush_filter() {
    let dir = tempdir().unwrap();

    let write_by_log2 = || {
        let mut log2 = OpenOptions::new()
            .create(true)
            .flush_filter(Some(|_, _| panic!("log2 flush filter should not run")))
            .open(dir.path())
            .unwrap();
        log2.append(b"log2").unwrap();
        log2.sync().unwrap();
    };

    let mut log1 = OpenOptions::new()
        .create(true)
        .flush_filter(Some(|ctx: &FlushFilterContext, bytes: &[u8]| {
            // "new" changes by log2 are visible.
            assert_eq!(ctx.log.iter().next().unwrap().unwrap(), b"log2");
            Ok(match bytes.len() {
                1 => FlushFilterOutput::Drop,
                2 => FlushFilterOutput::Replace(b"cc".to_vec()),
                4 => return Err(Box::new(DummyError("error"))),
                _ => FlushFilterOutput::Keep,
            })
        }))
        .open(dir.path())
        .unwrap();

    log1.append(b"a").unwrap(); // dropped
    log1.append(b"bb").unwrap(); // replaced to "cc"
    log1.append(b"ccc").unwrap(); // kept
    write_by_log2();
    log1.sync().unwrap();

    assert_eq!(
        log1.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        vec![&b"log2"[..], b"cc", b"ccc"]
    );

    log1.append(b"dddd").unwrap(); // error
    write_by_log2();
    log1.sync().unwrap_err();
}

/// Get a `Log` with index defined on first 8 bytes.
fn log_with_index(path: &Path, lag: u64) -> Log {
    let index_func = |_data: &[u8]| vec![IndexOutput::Reference(0..8)];
    let index_def = IndexDef::new("i", index_func).lag_threshold(lag);
    Log::open(path, vec![index_def]).unwrap()
}

/// Insert entries to a log
fn insert_entries(log: &mut Log, start: u64, n: u64) {
    for i in start..(start + n) {
        let buf: [u8; 8] = i.to_ne_bytes();
        log.append(&buf[..]).unwrap();
    }
}

#[test]
fn test_sync_fast_paths() {
    // Make sure various "sync" code paths do not lose data.
    //
    // Include these paths:
    //
    // - log1 and log2 are created.
    // - log1 writes (choice1)
    //   - 1: with index lag = 0
    //   - 2: with index lag = large value
    //   - 3: skip this step
    // - log1 sync()
    // - log2 writes (choice2)
    //   - 4: with index lag = 0
    //   - 5: with index lag = large value
    //   - 6: skip this step
    // - log2 sync()
    // - log1 sync()
    //
    // Examine log2 and log1 indexes by counting the entries in the log
    // and the index.

    const N: u64 = 1003;

    for choice1 in &[1, 2, 3] {
        for choice2 in &[4, 5, 6] {
            let dir = tempdir().unwrap();
            // Write a single entry to make the log non-empty.
            // So it's slightly more interesting.
            let mut log0 = log_with_index(dir.path(), 0);
            log0.sync().unwrap();

            let mut log1 = log_with_index(dir.path(), (choice1 - 1) << 29);
            let mut log2 = log_with_index(dir.path(), (choice2 - 4) << 29);
            let mut count = 0usize;

            if *choice1 < 3 {
                count += N as usize;
                insert_entries(&mut log1, 0, N);
            }
            log1.sync().unwrap();

            if *choice2 < 6 {
                count += (N as usize) * 2;
                insert_entries(&mut log2, N, N * 2);
            }
            log2.sync().unwrap();
            log1.sync().unwrap();

            let s = format!("(choices = {} {})", choice1, choice2);
            assert_eq!(
                log1.lookup_range(0, ..).unwrap().count(),
                count,
                "log1 index is incomplete {}",
                s
            );
            assert_eq!(
                log2.lookup_range(0, ..).unwrap().count(),
                count,
                "log2 index is incomplete {}",
                s
            );
            assert_eq!(log1.iter().count(), count, "log1 log is incomplete {}", s);
            assert_eq!(log2.iter().count(), count, "log2 log is incomplete {}", s);
        }
    }
}

#[test]
fn test_auto_sync_threshold() {
    let dir = tempdir().unwrap();
    let open_opts = OpenOptions::new().create(true).auto_sync_threshold(100);
    let mut log = open_opts.open(dir.path()).unwrap();
    log.append(vec![b'a'; 50]).unwrap();
    assert_eq!(log.iter_dirty().count(), 1);

    log.append(vec![b'b'; 50]).unwrap(); // trigger auto-sync
    assert_eq!(log.iter_dirty().count(), 0);
}

#[test]
fn test_sync_missing_meta() {
    let dir = tempdir().unwrap();
    let open_opts = OpenOptions::new().create(true);
    let mut log = open_opts.open(dir.path()).unwrap();
    log.append(vec![b'a'; 100]).unwrap();
    log.sync().unwrap();

    let mut log2 = open_opts.open(dir.path()).unwrap();
    fs::remove_file(dir.path().join(META_FILE)).unwrap();
    log2.sync().unwrap(); // pretend to be a no-op

    log2.append(vec![b'b'; 100]).unwrap();
    log2.sync().unwrap_err(); // an error
}

fn test_rebuild_indexes() {
    let dir = tempdir().unwrap();
    let open_opts = OpenOptions::new().create(true).index_defs(vec![
        IndexDef::new("key", |data| {
            vec![IndexOutput::Reference(0..data.len() as u64)]
        })
        .lag_threshold(1),
    ]);
    let mut log = open_opts.open(dir.path()).unwrap();

    log.append(b"abc").unwrap();
    log.flush().unwrap();

    log.append(b"def").unwrap();
    log.flush().unwrap();

    let dump_index = || {
        let index = index::OpenOptions::new()
            .open(dir.path().join("index2-key"))
            .unwrap();
        format!("{:?}", index)
    };

    let dump1 = dump_index();
    assert_eq!(
        dump1,
        "Index { len: 53, root: Disk[40] }\n\
         Disk[1]: InlineLeaf { key: Disk[2], link: Disk[5] }\n\
         Disk[2]: ExtKey { start: 18, len: 3 }\n\
         Disk[5]: Link { value: 12, next: None }\n\
         Disk[8]: Radix { link: None, 6: Disk[1] }\n\
         Disk[16]: Root { radix: Disk[8], meta: [21] }\n\
         Disk[21]: InlineLeaf { key: Disk[22], link: Disk[25] }\n\
         Disk[22]: ExtKey { start: 27, len: 3 }\n\
         Disk[25]: Link { value: 21, next: None }\n\
         Disk[28]: Radix { link: None, 1: Disk[1], 4: Disk[21] }\n\
         Disk[40]: Radix { link: None, 6: Disk[28] }\n\
         Disk[48]: Root { radix: Disk[40], meta: [30] }\n"
    );

    // If force is false, it is a no-op since the index passes the
    // checksum check.
    log.try_clone().unwrap().rebuild_indexes(false).unwrap();
    assert_eq!(dump_index(), dump1);

    // Setting force to true to rebuild the index.
    log.rebuild_indexes(true).unwrap();

    // The rebuilt index only contains one Root.
    assert_eq!(
        dump_index(),
        "Index { len: 40, root: Disk[27] }\n\
         Disk[1]: InlineLeaf { key: Disk[2], link: Disk[5] }\n\
         Disk[2]: ExtKey { start: 18, len: 3 }\n\
         Disk[5]: Link { value: 12, next: None }\n\
         Disk[8]: InlineLeaf { key: Disk[9], link: Disk[12] }\n\
         Disk[9]: ExtKey { start: 27, len: 3 }\n\
         Disk[12]: Link { value: 21, next: None }\n\
         Disk[15]: Radix { link: None, 1: Disk[1], 4: Disk[8] }\n\
         Disk[27]: Radix { link: None, 6: Disk[15] }\n\
         Disk[35]: Root { radix: Disk[27], meta: [30] }\n"
    );

    // The index actually works (checksum table is consistent).
    let log = open_opts.open(dir.path()).unwrap();
    assert_eq!(log.lookup(0, b"abc").unwrap().count(), 1);
    assert_eq!(log.lookup(0, b"def").unwrap().count(), 1);
    assert_eq!(log.lookup(0, b"xyz").unwrap().count(), 0);
}

pub(crate) fn pwrite(path: &Path, offset: i64, data: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .read(true)
        .open(path)
        .unwrap();
    if offset < 0 {
        file.seek(SeekFrom::End(offset)).unwrap();
    } else {
        file.seek(SeekFrom::Start(offset as u64)).unwrap();
    }
    file.write_all(data).unwrap();
}

#[test]
fn test_repair() {
    let dir = tempdir().unwrap();
    {
        let mut log = Log::open(dir.path(), Vec::new()).unwrap();
        log.append(b"abc").unwrap();
        log.append(b"def").unwrap();
        log.append(b"ghi").unwrap();
        log.flush().unwrap();
    }

    // Corrupt the log by changing the last byte.
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .read(true)
            .open(dir.path().join(PRIMARY_FILE))
            .unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        file.write_all(b"x").unwrap();
    }

    // Reading entries would error out.
    {
        let log = Log::open(dir.path(), Vec::new()).unwrap();
        assert!(log.iter().nth(2).unwrap().is_err());
    }

    // Repair.
    {
        OpenOptions::new().repair(dir.path()).unwrap();
    }

    // Reading entries is recovered. But we lost one entry.
    let mut log = Log::open(dir.path(), Vec::new()).unwrap();
    assert_eq!(
        log.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        vec![b"abc", b"def"]
    );

    // Writing is recovered.
    log.append(b"pqr").unwrap();
    log.flush().unwrap();

    let log = Log::open(dir.path(), Vec::new()).unwrap();
    assert_eq!(
        log.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        vec![b"abc", b"def", b"pqr"]
    );
}

#[test]
fn test_repair_on_open() {
    use crate::OpenWithRepair;

    let dir = tempdir().unwrap();
    let path = dir.path();
    let opts = OpenOptions::new().create(true);

    // Prepare some data.
    let mut log = opts.open(path).unwrap();
    log.append(b"abc").unwrap();
    log.flush().unwrap();

    // Corrupt the log by breaking the meta file.
    let meta_path = path.join(META_FILE);
    utils::atomic_write(meta_path, b"xxx", false).unwrap();

    // Opening the log errors out.
    opts.open(path).unwrap_err();

    // Auto repair fails because it detects active reader.
    opts.open_with_repair(path).unwrap_err();
    opts.open(path).unwrap_err();

    // Drop the active reader.
    drop(log);

    // Opening with auto repair succeeds.
    let log = opts.open_with_repair(path).unwrap();

    // Reading entries is fine.
    assert_eq!(
        log.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        vec![b"abc"]
    );
}

#[test]
fn test_repair_noop() {
    // Repair does nothing if the Log can be read out without issues.
    let dir = tempdir().unwrap();
    let mut log = Log::open(dir.path(), Vec::new()).unwrap();
    log.append(b"abc").unwrap();
    log.append(b"def").unwrap();
    log.append(b"ghi").unwrap();
    log.flush().unwrap();

    let meta_before = LogMetadata::read_file(dir.path().join(META_FILE)).unwrap();
    OpenOptions::new().repair(dir.path()).unwrap();
    let meta_after = LogMetadata::read_file(dir.path().join(META_FILE)).unwrap();
    assert_eq!(meta_before, meta_after);
}

#[test]
fn test_repair_and_delete_content() {
    let dir = tempdir().unwrap();
    let path = dir.path();
    let open_opts = OpenOptions::new().create(true).index_defs(vec![
        IndexDef::new("c", |_| vec![IndexOutput::Reference(0..1)]).lag_threshold(5000),
    ]);

    let long_lived_log = RefCell::new(open_opts.open(()).unwrap());
    let open = || open_opts.open(path);
    let corrupt = |name: &str, offset: i64| pwrite(&path.join(name), offset, b"cc");
    let truncate = |name: &str| fs::write(path.join(name), "garbage").unwrap();
    let delete = |name: &str| fs::remove_file(path.join(name)).unwrap();
    let index_file = "index2-c";
    let append = || {
        let mut log = open().unwrap();
        log.append(&[b'x'; 50_000][..]).unwrap();
        log.append(&[b'y'; 50_000][..]).unwrap();
        log.append(&[b'z'; 50_000][..]).unwrap();
        log.sync().unwrap();
    };
    let count = || -> crate::Result<(usize, usize)> {
        let log = open()?;
        let log_len = log.iter().collect::<Result<Vec<_>, _>>()?.len();
        let mut index_len = 0;
        for key in [b"x", b"y", b"z"] {
            let iter = log.lookup(0, key)?;
            index_len += iter.into_vec()?.len();
        }
        Ok((log_len, index_len))
    };
    let verify_len = |len: usize| {
        let (log_len, index_len) = count().unwrap();
        assert_eq!(log_len, len);
        assert_eq!(index_len, len);
    };
    let verify_corrupted = || {
        let err = count().unwrap_err();
        assert!(err.is_corruption(), "not a corruption:\n {:?}", err);
    };
    let try_trigger_sigbus = || {
        // Check no SIGBUS
        let log = long_lived_log.borrow();
        match log.lookup(0, "z") {
            Err(_) => {} // okay - not SIGBUS
            Ok(iter) => match iter.into_vec() {
                Err(_) => {} // okay - not SIGBUS
                Ok(_) => {}  // okay - not SIGBUS
            },
        }
        // Check 'sync' on a long-lived log will load the right data and
        // resolve errors.
        let mut cloned_log = log.try_clone().unwrap();
        cloned_log.sync().unwrap();
        let _ = cloned_log.lookup(0, "z").unwrap().into_vec().unwrap();
    };
    let repair = || {
        let message = open_opts.repair(path).unwrap();
        try_trigger_sigbus();
        message
            .lines()
            // Remove 'Backed up' lines since they have dynamic file names.
            .filter(|l| !l.contains("Backed up"))
            .filter(|l| !l.contains("Processing"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Repair is a no-op if log and indexes pass integirty check.
    append();
    verify_len(3);
    assert_eq!(
        repair(),
        r#"Verified 3 entries, 150048 bytes in log
Index "c" passed integrity check"#
    );

    append();
    verify_len(6);
    assert_eq!(
        repair(),
        r#"Verified 6 entries, 300084 bytes in log
Index "c" passed integrity check"#
    );

    // Prepare long-lived log for SIGBUS check
    // (skip on Windows, since mmap makes it impossible to replace files)
    if cfg!(unix) {
        long_lived_log.replace(open().unwrap());
    }

    // Corrupt the end of log
    corrupt(PRIMARY_FILE, -1);
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Verified first 5 entries, 250072 of 300084 bytes in log
Reset log size to 250072
Index "c" is incompatible with (truncated) log
Rebuilt index "c""#
    );
    verify_len(5);

    // Corrupt the middle of log
    corrupt(PRIMARY_FILE, 125000);
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Verified first 2 entries, 100036 of 250072 bytes in log
Reset log size to 100036
Index "c" is incompatible with (truncated) log
Rebuilt index "c""#
    );
    verify_len(2);

    append();
    verify_len(5);

    // Change the beginning of log
    corrupt(PRIMARY_FILE, 1);
    verify_len(5);
    assert_eq!(
        repair(),
        r#"Fixed header in log
Verified 5 entries, 250072 bytes in log
Index "c" passed integrity check"#
    );

    // Corrupt the end of index
    corrupt(index_file, -1);
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Verified 5 entries, 250072 bytes in log
Rebuilt index "c""#
    );
    verify_len(5);

    // Corrupt the beginning of index
    corrupt(index_file, 1);
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Verified 5 entries, 250072 bytes in log
Rebuilt index "c""#
    );
    verify_len(5);

    // Replace index with garbage
    truncate(index_file);
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Verified 5 entries, 250072 bytes in log
Rebuilt index "c""#
    );
    verify_len(5);

    // Replace log with garbage
    truncate(PRIMARY_FILE);
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Fixed header in log
Extended log to 250072 bytes required by meta
Verified first 0 entries, 12 of 250072 bytes in log
Reset log size to 12
Index "c" is incompatible with (truncated) log
Rebuilt index "c""#
    );
    verify_len(0);

    append();
    verify_len(3);

    // Delete index
    delete(index_file);
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Verified 3 entries, 150048 bytes in log
Rebuilt index "c""#
    );
    verify_len(3);

    // Delete log
    delete(PRIMARY_FILE);
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Fixed header in log
Extended log to 150048 bytes required by meta
Verified first 0 entries, 12 of 150048 bytes in log
Reset log size to 12
Index "c" is incompatible with (truncated) log
Rebuilt index "c""#
    );
    verify_len(0);

    // Corrupt the middle of index. This test wants to be able
    // to make it okay to open Index, but not okay to use it at
    // some random place. The index checksum chunk size is 1MB
    // so the index has to be a few MBs to be able to pass checksum
    // check at Index open time.
    // To do that, insert a lot entries to the log.
    //
    // Practically, this should show "Index .. failed integrity check".
    let append_many_entries = || {
        let mut log = open().unwrap();
        for _ in 0..200_000 {
            log.append(&[b'z'; 1][..]).unwrap();
        }
        log.sync().unwrap();
    };
    append_many_entries();
    corrupt(index_file, -1_000_000);
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Verified 200000 entries, 1400012 bytes in log
Index "c" failed integrity check
Rebuilt index "c""#
    );
    verify_len(200000);

    // Corrupt meta
    crate::utils::atomic_write(path.join(META_FILE), b"xxx", false).unwrap();
    corrupt(PRIMARY_FILE, 1000);
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Rebuilt metadata
Verified first 141 entries, 999 of 1400012 bytes in log
Reset log size to 999
Rebuilt index "c""#
    );
    verify_len(141);

    crate::utils::atomic_write(path.join(META_FILE), b"yyy", false).unwrap();
    verify_corrupted();
    assert_eq!(
        repair(),
        r#"Rebuilt metadata
Verified first 141 entries, 999 of 1400012 bytes in log
Reset log size to 999
Rebuilt index "c""#
    );
    verify_len(141);

    // Delete meta - as if the log directory does not exist.
    delete(META_FILE);
    assert_eq!(
        repair(),
        r#"Rebuilt metadata
Verified first 141 entries, 999 of 1400012 bytes in log
Reset log size to 999
Rebuilt index "c""#
    );
    verify_len(141);

    let len = |name: &str| path.join(name).metadata().unwrap().len();
    let append = || {
        let mut log = open().unwrap();
        log.append(&[b'x'; 50_000][..]).unwrap();
        log.append(&[b'y'; 50_000][..]).unwrap();
        log.append(&[b'z'; 50_000][..]).unwrap();
        log.sync().unwrap();
        assert_eq!(len(PRIMARY_FILE), PRIMARY_START_OFFSET + 150036);
        assert_eq!(len(index_file), 100);
    };
    let delete_content = || {
        open_opts.delete_content(path).unwrap();
        assert_eq!(len(PRIMARY_FILE), PRIMARY_START_OFFSET);
        assert_eq!(len(index_file), 25);
        // Check SIGBUS
        try_trigger_sigbus();
        // Check log is empty
        verify_len(0);
    };

    // 'dir' does not exist - delete_content creates the log
    fs::remove_dir_all(path).unwrap();
    delete_content();

    // Normal log
    append();
    if cfg!(unix) {
        long_lived_log.replace(open().unwrap());
    }
    delete_content();

    // Corrupt log
    append();
    corrupt(PRIMARY_FILE, -75_000);
    delete_content();

    // Corrupt index
    append();
    corrupt(index_file, -10);
    delete_content();

    // Corrupt log and index
    append();
    corrupt(PRIMARY_FILE, -25_000);
    corrupt(index_file, -10);
    delete_content();

    // Deleted various files
    delete(index_file);
    delete_content();

    delete(PRIMARY_FILE);
    delete_content();

    delete(META_FILE);
    delete_content();
}

#[test]
fn test_zero_data() {
    // Emulating the case where meta was written, but log was zeroed out.
    // This should be captured by checksums.
    let dir = tempdir().unwrap();
    let mut log = Log::open(dir.path(), Vec::new()).unwrap();
    log.append(b"abcd").unwrap();
    log.flush().unwrap();

    let len_before = dir.path().join(PRIMARY_FILE).metadata().unwrap().len();
    log.append(b"efgh").unwrap();
    log.flush().unwrap();

    let len_after = dir.path().join(PRIMARY_FILE).metadata().unwrap().len();

    // Zero-out the second entry
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .read(true)
            .open(dir.path().join(PRIMARY_FILE))
            .unwrap();
        file.seek(SeekFrom::Start(len_before)).unwrap();
        file.write_all(&vec![0; (len_after - len_before) as usize])
            .unwrap();
    }

    let log = Log::open(dir.path(), Vec::new()).unwrap();
    assert!(log.iter().any(|e| e.is_err()));
}

#[cfg(unix)]
#[test]
fn test_non_append_only() {
    // Test non-append-only changes can be detected by epoch change.
    //
    // In this test, we create 2 logs with different content. Then swap
    // those 2 logs and call sync.
    //
    // This test requires renaming files while mmap is present. That
    // cannot be done in Windows.
    //
    // This test should fail if utils::rand_u64 returns a constant.
    let dir = tempdir().unwrap();

    let indexes = vec![IndexDef::new("key1", index_ref).lag_threshold(1)];
    let open_opts = OpenOptions::new().create(true).index_defs(indexes);

    // Prepare the first log.
    let mut log1 = open_opts.open(dir.path().join("1")).unwrap();
    for b in 0..10 {
        log1.append(&[b; 7][..]).unwrap();
    }
    log1.flush().unwrap();
    for b in 30..40 {
        log1.append(&[b; 21][..]).unwrap();
    }

    // Prepare the second log
    let mut log2 = open_opts.open(dir.path().join("2")).unwrap();
    for b in 20..30 {
        log2.append(&[b; 21][..]).unwrap();
    }
    log2.flush().unwrap();
    for b in 10..20 {
        log2.append(&[b; 7][..]).unwrap();
    }

    // Rename to emulate the non-append-only change.
    fs::rename(dir.path().join("1"), dir.path().join("temp")).unwrap();
    fs::rename(dir.path().join("2"), dir.path().join("1")).unwrap();
    fs::rename(dir.path().join("temp"), dir.path().join("2")).unwrap();

    log1.sync().unwrap();
    log2.sync().unwrap();

    // Check their content.
    fn check_log(log: &Log, range: Range<u8>, len: usize) {
        assert_eq!(
            log.iter().map(|b| b.unwrap().to_vec()).collect::<Vec<_>>(),
            range.clone().map(|i| vec![i; len]).collect::<Vec<_>>(),
        );
        assert_eq!(
            log.lookup_range(0, ..)
                .unwrap()
                .flat_map(|e| e.unwrap().1.into_vec().unwrap())
                .map(|b| b.to_vec())
                .collect::<Vec<_>>(),
            range.map(|i| vec![i; len]).collect::<Vec<_>>(),
        );
    }

    check_log(&log1, 20..40, 21);
    check_log(&log2, 0..20, 7);

    let log1 = open_opts.open(dir.path().join("1")).unwrap();
    let log2 = open_opts.open(dir.path().join("2")).unwrap();

    check_log(&log1, 20..40, 21);
    check_log(&log2, 0..20, 7);
}

#[test]
fn test_clear_dirty() {
    for lag in &[0, 1000] {
        let dir = tempdir().unwrap();
        let mut log = log_with_index(dir.path(), *lag);
        log.append([b'a'; 10]).unwrap();
        log.sync().unwrap();
        log.append([b'b'; 10]).unwrap();
        assert_eq!(log.lookup_range(0, ..).unwrap().count(), 2);

        log.clear_dirty().unwrap();
        assert_eq!(
            log.iter().collect::<Result<Vec<_>, _>>().unwrap(),
            vec![[b'a'; 10]],
        );
        assert_eq!(log.lookup_range(0, ..).unwrap().count(), 1);
    }
}

#[test]
fn test_clone() {
    for lag in &[0, 1000] {
        let dir = tempdir().unwrap();
        let mut log = log_with_index(dir.path(), *lag);
        log.append([b'a'; 10]).unwrap();
        log.sync().unwrap();
        log.append([b'b'; 10]).unwrap();

        let mut log2 = log.try_clone().unwrap();
        assert_eq!(log2.iter().collect::<Result<Vec<_>, _>>().unwrap().len(), 2);
        assert_eq!(log2.lookup_range(0, ..).unwrap().count(), 2);
        // Check the external key buffer works after clone.
        log2.append(&[b'c'; 40960][..]).unwrap();
        log2.append(&[b'd'; 40960][..]).unwrap();

        let mut log2 = log.try_clone_without_dirty().unwrap();
        assert_eq!(log2.iter().collect::<Result<Vec<_>, _>>().unwrap().len(), 1);
        assert_eq!(log2.lookup_range(0, ..).unwrap().count(), 1);
        log2.append(&[b'e'; 40960][..]).unwrap();
        log2.append(&[b'f'; 40960][..]).unwrap();
    }
}

#[test]
fn test_is_changed_on_disk() {
    let dir = tempdir().unwrap();
    let log_path = dir.path().join("log");
    let mut log1 = Log::open(&log_path, Vec::new()).unwrap();
    let mut log2 = Log::open(&log_path, Vec::new()).unwrap();

    assert!(!log1.is_changed_on_disk());
    assert!(!log2.is_changed_on_disk());

    // no-op sync() does not set is_changed().
    log1.sync().unwrap();
    assert!(!log2.is_changed_on_disk());

    // change before flush does not set is_changed().
    log1.append([b'a'; 10]).unwrap();

    assert!(!log1.is_changed_on_disk());
    assert!(!log2.is_changed_on_disk());

    // sync() does not set is_changed().
    log1.sync().unwrap();
    assert!(!log1.is_changed_on_disk());

    // log2 should be able to detect the on-disk change from log1.
    assert!(log2.is_changed_on_disk());

    // is_changed() does not clear is_changed().
    assert!(log2.is_changed_on_disk());

    // read-only sync() should clear is_changed().
    log2.sync().unwrap();
    assert!(!log2.is_changed_on_disk());
    // ... and not set other Logs' is_changed().
    assert!(!log1.is_changed_on_disk());

    log2.append([b'a'; 10]).unwrap();
    log2.sync().unwrap();

    // log1 should be able to detect the on-disk change from log2.
    assert!(log1.is_changed_on_disk());

    // read-write sync() should clear is_changed().
    log1.append([b'a'; 10]).unwrap();
    log1.sync().unwrap();
    assert!(!log1.is_changed_on_disk());
}

#[test]
fn test_multithread_sync() {
    let dir = tempdir().unwrap();

    const THREAD_COUNT: u8 = 30;

    // Release mode runs much faster.
    const WRITE_COUNT_PER_THREAD: u8 = if cfg!(debug_assertions) { 30 } else { 150 };

    // Some indexes. They have different lag_threshold.
    fn index_copy(data: &[u8]) -> Vec<IndexOutput> {
        vec![IndexOutput::Owned(data.to_vec().into_boxed_slice())]
    }
    let indexes = vec![
        IndexDef::new("key1", index_ref).lag_threshold(10),
        IndexDef::new("key2", index_ref).lag_threshold(50),
        IndexDef::new("key3", index_ref).lag_threshold(100),
        IndexDef::new("key4", index_copy).lag_threshold(10),
        IndexDef::new("key5", index_copy).lag_threshold(50),
        IndexDef::new("key6", index_copy).lag_threshold(100),
    ];
    let index_len = indexes.len();
    let open_opts = OpenOptions::new().create(true).index_defs(indexes);

    let barrier = Arc::new(std::sync::Barrier::new(THREAD_COUNT as usize));
    let threads: Vec<_> = (0..THREAD_COUNT)
        .map(|i| {
            let barrier = barrier.clone();
            let open_opts = open_opts.clone();
            let path = dir.path().to_path_buf();
            std::thread::spawn(move || {
                barrier.wait();
                let non_lag_open_opts = open_opts.clone().with_zero_index_lag();
                let mut log = open_opts.open(&path).unwrap();
                for j in 1..=WRITE_COUNT_PER_THREAD {
                    let buf = [i, j];
                    // Pick 1/4 threads to do "no_lag" opens.
                    if i % 4 == 0 {
                        if j % 8 == 0 {
                            // Reload. This might trigger writes (updating lagging indexes).
                            log.sync().unwrap();
                            log = non_lag_open_opts.clone().open(&path).unwrap();
                        } else if j % 8 == 4 {
                            log.sync().unwrap();
                            log = open_opts.clone().open(&path).unwrap();
                        }
                    }
                    log.append(buf).unwrap();
                    if j % (i + 1) == 0 || j == WRITE_COUNT_PER_THREAD {
                        log.sync().unwrap();
                        // Verify that the indexes match the entries.
                        let mut entries =
                            log.iter().map(|d| d.unwrap().to_vec()).collect::<Vec<_>>();
                        entries.sort_unstable();
                        for index_id in 0..index_len {
                            let mut entries_index_keys = Vec::with_capacity(entries.len());
                            let mut entries_index_values = Vec::with_capacity(entries.len());
                            for entry_iter in log.lookup_range(index_id, ..).unwrap() {
                                let (key, value_iter) = entry_iter.unwrap();
                                entries_index_keys.push(key.as_ref().to_vec());
                                for value in value_iter {
                                    entries_index_values.push(value.unwrap().to_vec());
                                }
                            }
                            assert_eq!(entries, entries_index_keys);
                            assert_eq!(entries, entries_index_values);
                        }
                    }
                }
            })
        })
        .collect();

    // Wait for them.
    for thread in threads {
        thread.join().expect("joined");
    }

    // Check how many entries were written.
    let log = open_opts.open(dir.path()).unwrap();
    let count = log.iter().count() as u64;
    assert_eq!(count, THREAD_COUNT as u64 * WRITE_COUNT_PER_THREAD as u64);
}

#[test]
fn test_wait_for_changes() {
    let dir = tempdir().unwrap();
    let (tx, rx) = channel::<i32>();
    let mut log = Log::open(&dir, Vec::new()).unwrap();

    let mut wait = crate::log::Wait::from_log(&log).unwrap();
    std::thread::spawn({
        move || {
            wait.wait_for_change().unwrap();
            tx.send(101).unwrap();
            wait.wait_for_change().unwrap();
            tx.send(102).unwrap();
        }
    });

    std::thread::sleep(Duration::from_millis(110));
    assert!(rx.try_recv().is_err());

    log.append(b"1").unwrap();
    log.sync().unwrap();

    assert_eq!(rx.recv().unwrap(), 101);

    log.append(b"2").unwrap();
    log.sync().unwrap();

    assert_eq!(rx.recv().unwrap(), 102);
}

fn index_ref(data: &[u8]) -> Vec<IndexOutput> {
    vec![IndexOutput::Reference(0..data.len() as u64)]
}

quickcheck! {
    fn test_roundtrip_entries(entries: Vec<(Vec<u8>, bool, bool)>) -> bool {
        let dir = tempdir().unwrap();
        let mut log = Log::open(dir.path(), Vec::new()).unwrap();
        let mut log_mem = OpenOptions::new().open(()).unwrap();
        for &(ref data, flush, reload) in &entries {
            log.append(data).expect("append");
            log_mem.append(data).expect("append");
            if flush {
                log.sync().expect("flush");
                if reload {
                    log = Log::open(dir.path(), Vec::new()).unwrap();
                }
            }
        }
        let retrieved: Vec<Vec<u8>> = log.iter().map(|v| v.unwrap().to_vec()).collect();
        let retrieved_mem: Vec<Vec<u8>> = log_mem.iter().map(|v| v.unwrap().to_vec()).collect();
        let entries: Vec<Vec<u8>> = entries.iter().map(|v| v.0.clone()).collect();
        retrieved == entries && retrieved_mem == entries
    }
}
