/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Regression tests for deserializing BSER payloads with a negative
//! bytestring/utf8string length. The wire length is a signed integer and used
//! to index into the buffer, so a negative value must be rejected instead of
//! being reinterpreted as a huge `usize`.

use std::io::Cursor;

use serde_bser::de::from_reader;
use serde_bser::de::from_slice;

fn negative_len_bytestring() -> Vec<u8> {
    // BSER magic + capabilities + PDU length (INT8 = 3) + a bytestring whose
    // length is INT8 = -1 (0xFF).
    let mut msg = vec![0x00, 0x02];
    msg.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    msg.extend_from_slice(&[0x03, 0x03]);
    msg.extend_from_slice(&[0x02, 0x03, 0xFF]);
    msg
}

#[test]
fn negative_bytestring_length_is_rejected() {
    let msg = negative_len_bytestring();
    let r: Result<String, _> = from_slice(&msg);
    assert!(
        r.is_err(),
        "expected a deserialization error for a negative bytestring length, got {:?}",
        r
    );
}

#[test]
fn negative_bytestring_length_is_rejected_from_reader() {
    let msg = negative_len_bytestring();
    let r: Result<String, _> = from_reader(Cursor::new(msg));
    assert!(
        r.is_err(),
        "expected a deserialization error for a negative bytestring length, got {:?}",
        r
    );
}
