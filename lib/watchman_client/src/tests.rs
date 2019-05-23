// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate bytes;

use self::bytes::Buf;
use crate::queries::*;
use serde_bser;
use serde_json;
use std::io::Cursor;

/// Parsing tests

#[test]
fn test_get_sockname_response() {
    let bser_v2 = b"\x00\x02\x00\x00\x00\x00\x03\x5d\x01\x03\x02\x02\x03\x07version\x02\x03\x054.9.1\x02\x03\x08sockname\x02\x03\x3a/opt/facebook/watchman/var/run/watchman/liubovd-state/sock";
    let json_str = serde_json::to_string(&json!(
    {
        "version": "4.9.1",
        "sockname": "/opt/facebook/watchman/var/run/watchman/liubovd-state/sock",
    }
    ))
    .unwrap();
    let (reader1, reader2) = (
        Cursor::new(bser_v2.to_vec()).reader(),
        Cursor::new(json_str.into_bytes()).reader(),
    );
    let decoded1: QueryResponse = serde_bser::from_reader(reader1).unwrap();
    let decoded2: QueryResponse = serde_json::from_reader(reader2).unwrap();
    assert_eq!(decoded1, decoded2);
}

#[test]
fn test_watch_project_response() {
    let bser_v2 = b"\x00\x02\x00\x00\x00\x00\x03L\x01\x03\x03\x02\x03\x07watcher\x02\x03\x08fsevents\x02\x03\x05watch\x02\x03\x17/Users/liubovd/fbsource\x02\x03\x07version\x0d\x03\x054.9.1";
    let json_str = serde_json::to_string(&json!(
    {
        "version": "4.9.1",
        "watch": "/Users/liubovd/fbsource",
        "watcher": "fsevents"
    }
    ))
    .unwrap();
    let (reader1, reader2) = (
        Cursor::new(bser_v2.to_vec()).reader(),
        Cursor::new(json_str.into_bytes()).reader(),
    );
    let decoded1: QueryResponse = serde_bser::from_reader(reader1).unwrap();
    let decoded2: QueryResponse = serde_json::from_reader(reader2).unwrap();
    assert_eq!(decoded1, decoded2);
}

#[test]
fn test_query_response_with_multiple_fields() {
    let bser_v2 = b"\x00\x02\x00\x00\x00\x00\x04\x20\x01\x01\x03\x04\x02\x03\x05files\x0b\x00\x03\x05\x0d\x03\x04mode\x0d\x03\x05mtime\x0d\x03\x04size\x0d\x03\x06exists\x0d\x03\x04name\x03\x02\x05\xa4\x81\x00\x00\x05\xd0\x06\x03\x5b\x04\xe4\x06\x08\x02\x03Cfbcode/scm/hg/lib/hg_watchman_client/tester/target/release/tester.d\x04\xedA\x05\xce\x06\x03\x5b\x04\x60\x01\x08\x02\x03\x3afbcode/scm/hg/lib/hg_watchman_client/tester/target/release\x02\x03\x05clock\x0d\x03\x1ac\x3a1525428959\x3a45796\x3a2\x3a86773\x02\x03\x11is_fresh_instance\x09\x02\x03\x07version\x0d\x03\x054.9.1";
    let json_str = serde_json::to_string(&json!(
        {
            "version": "4.9.1",
            "files": [
                {
                    "exists": true,
                    "mode": 33188,
                    "name": "fbcode/scm/hg/lib/hg_watchman_client/tester/target/release/tester.d",
                    "size": 1764,
                    "mtime": 1526925008
                },
                {
                    "exists": true,
                    "mode": 16877,
                    "name": "fbcode/scm/hg/lib/hg_watchman_client/tester/target/release",
                    "size": 352,
                    "mtime": 1526925006
                }
            ],
            "clock": "c:1525428959:45796:2:86773",
            "is_fresh_instance": false
        }
    ))
    .unwrap();
    let (reader1, reader2) = (
        Cursor::new(bser_v2.to_vec()).reader(),
        Cursor::new(json_str.into_bytes()).reader(),
    );
    let decoded1: QueryResponse = serde_bser::from_reader(reader1).unwrap();
    let decoded2: QueryResponse = serde_json::from_reader(reader2).unwrap();
    assert_eq!(decoded1, decoded2);
}

#[test]
fn test_query_response_with_names_only() {
    let bser_v2 = b"\x00\x02\x00\x00\x00\x00\x04z\x02\x01\x03\x04\x02\x03\x05files\x00\x03\x06\x02\x03\x3afbcode/scm/hg/lib/hg_watchman_client/tester/target/release\x02\x03_fbcode/scm/hg/lib/hg_watchman_client/tester/target/release/.fingerprint/tester-3b95fff145f10c94\x02\x03\x3ffbcode/scm/hg/lib/hg_watchman_client/tester/target/release/deps\x02\x03kfbcode/scm/hg/lib/hg_watchman_client/tester/target/release/.fingerprint/hg_watchman_client-fb8127abf8b7beff\x02\x03hfbcode/scm/hg/lib/hg_watchman_client/tester/target/release/.fingerprint/watchman_client-a52711dccc417ac5\x02\x03cfbcode/scm/hg/lib/hg_watchman_client/tester/target/release/.fingerprint/serde_bser-90a32427b4db9fb7\x02\x03\x05clock\x0d\x03\x1ac\x3a1525428959\x3a45796\x3a2\x3a86580\x02\x03\x11is_fresh_instance\x09\x02\x03\x07version\x0d\x03\x054.9.1";
    let json_str = serde_json::to_string(&json!(
        {
            "version": "4.9.1",
            "files": [
            "fbcode/scm/hg/lib/hg_watchman_client/tester/target/release",
            "fbcode/scm/hg/lib/hg_watchman_client/tester/target/release/.fingerprint/tester-3b95fff145f10c94",
            "fbcode/scm/hg/lib/hg_watchman_client/tester/target/release/deps",
            "fbcode/scm/hg/lib/hg_watchman_client/tester/target/release/.fingerprint/hg_watchman_client-fb8127abf8b7beff",
            "fbcode/scm/hg/lib/hg_watchman_client/tester/target/release/.fingerprint/watchman_client-a52711dccc417ac5",
            "fbcode/scm/hg/lib/hg_watchman_client/tester/target/release/.fingerprint/serde_bser-90a32427b4db9fb7"
            ],
            "clock": "c:1525428959:45796:2:86580",
            "is_fresh_instance": false
        }
    )).unwrap();
    let (reader1, reader2) = (
        Cursor::new(bser_v2.to_vec()).reader(),
        Cursor::new(json_str.into_bytes()).reader(),
    );
    let decoded1: QueryResponse = serde_bser::from_reader(reader1).unwrap();
    let decoded2: QueryResponse = serde_json::from_reader(reader2).unwrap();
    assert_eq!(decoded1, decoded2);
}

#[test]
fn test_state_enter_response() {
    let bser_v2 = b"\x00\x02\x00\x00\x00\x00\x03S\x01\x03\x03\x02\x03\x0bstate-enter\x0d\x03\x0chg.filemerge\x02\x03\x04root\x02\x03\x17/Users/liubovd/fbsource\x02\x03\x07version\x0d\x03\x054.9.1";
    let json_str = serde_json::to_string(&json!(
        {
            "version": "4.9.1",
            "root": "/Users/liubovd/fbsource",
            "state-enter": "hg.filemerge"
        }
    ))
    .unwrap();
    let (reader1, reader2) = (
        Cursor::new(bser_v2.to_vec()).reader(),
        Cursor::new(json_str.into_bytes()).reader(),
    );
    let decoded1: QueryResponse = serde_bser::from_reader(reader1).unwrap();
    let decoded2: QueryResponse = serde_json::from_reader(reader2).unwrap();
    assert_eq!(decoded1, decoded2);
}

#[test]
fn test_state_leave_response() {
    let bser_v2 = b"\x00\x02\x00\x00\x00\x00\x03S\x01\x03\x03\x02\x03\x0bstate-leave\x0d\x03\x0chg.filemerge\x02\x03\x04root\x02\x03\x17/Users/liubovd/fbsource\x02\x03\x07version\x0d\x03\x054.9.1";
    let json_str = serde_json::to_string(&json!(
    {
        "version": "4.9.1",
        "root": "/Users/liubovd/fbsource",
        "state-leave": "hg.filemerge"
    }
    ))
    .unwrap();
    let (reader1, reader2) = (
        Cursor::new(bser_v2.to_vec()).reader(),
        Cursor::new(json_str.into_bytes()).reader(),
    );
    let decoded1: QueryResponse = serde_bser::from_reader(reader1).unwrap();
    let decoded2: QueryResponse = serde_json::from_reader(reader2).unwrap();
    assert_eq!(decoded1, decoded2);
}
