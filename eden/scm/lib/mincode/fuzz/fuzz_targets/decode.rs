/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Decode fuzzer bytes with `mincode::deserialize`.
//!
//! The input is fed to the deserializer raw rather than as a corruption of a
//! valid encoding. mincode has no magic, no header and no checksum: the first
//! thing read for every shape here is a VLQ length at offset 0, and any byte
//! below 0x80 is a complete VLQ. A random string is therefore already a
//! syntactically plausible encoding, and coverage feedback grows valid
//! prefixes on its own. Seeding from one valid value would instead bias every
//! input toward that value's field sizes, which is the opposite of what is
//! wanted from a format whose only gate is the target type.
//!
//! The first input byte selects the shape. All three share one decoder, so
//! splitting them into separate binaries would triple build and run cost
//! without isolating anything.

#![no_main]

use std::collections::BTreeMap;
use std::hint::black_box;

use libfuzzer_sys::fuzz_target;
use serde::Deserialize;
use serde::Serialize;

mod shapes;

use crate::shapes::LfsPointer;
use crate::shapes::Root;

/// Payload bytes one input may carry. libfuzzer defaults to 4096; the cap only
/// matters when `-max_len` is raised, and keeps a single input from turning
/// into a multi-megabyte decode.
const MAX_PAYLOAD: usize = 1 << 16;

/// Shapes with no on-disk consumer, so that a `Deserializer` method only they
/// reach is still covered.
///
/// `char` is a scalar field and never an element of a collection: the ASCII
/// branch of `deserialize_char` returns without advancing the cursor, so the
/// length of a `Vec<char>` would be bounded by nothing the input has to
/// supply. Every other collection element type below consumes at least one
/// byte, which is what bounds this target's per-input work by the input
/// length.
#[derive(Serialize, Deserialize)]
struct Wide<'a> {
    flag: bool,
    byte: u8,
    short: u16,
    word: u32,
    signed_byte: i8,
    signed: i64,
    single: f32,
    double: f64,
    letter: char,
    #[serde(borrow)]
    raw: &'a [u8],
    #[serde(borrow)]
    text: &'a str,
    nested: Vec<Vec<String>>,
    table: BTreeMap<u64, Vec<u8>>,
    optional: Option<u64>,
    tagged: Tag,
    pair: Pair,
    tuple: (u16, String),
    unit: Unit,
}

#[derive(Serialize, Deserialize)]
enum Tag {
    Nothing,
    One(u64),
    Two(u8, String),
    Named { id: u32, label: String },
}

#[derive(Serialize, Deserialize)]
struct Pair(u32, String);

#[derive(Serialize, Deserialize)]
struct Unit;

/// Decode, then read every field of the result by encoding it again.
///
/// Reading is what matters for the borrowed fields: mincode hands back `&[u8]`
/// and `&str` carved out of the input, so a slice that got past the bounds
/// check in `read_slice` is only visible to the sanitizers once its bytes are
/// touched. Holding the value in a `black_box` would pin the pointers and
/// leave the bytes behind them unread.
fn decode<'de, T>(bytes: &'de [u8])
where
    T: Deserialize<'de> + Serialize,
{
    let Ok(value) = mincode::deserialize::<T>(bytes) else {
        return;
    };
    black_box(mincode::serialize(&value).is_ok());
}

fuzz_target!(|input: &[u8]| {
    let Some((&selector, payload)) = input.split_first() else {
        return;
    };
    if payload.len() > MAX_PAYLOAD {
        return;
    }
    match selector % 3 {
        0 => decode::<Root>(payload),
        1 => decode::<LfsPointer>(payload),
        _ => decode::<Wide<'_>>(payload),
    }
});
