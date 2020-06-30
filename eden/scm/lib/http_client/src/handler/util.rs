/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::{self, Utf8Error};

/// Attempt to parse a header as UTF-8 and split it on colons.
pub(super) fn split_header(header: &[u8]) -> Result<(&str, &str), Utf8Error> {
    let header = str::from_utf8(header)?.splitn(2, ':').collect::<Vec<_>>();
    Ok(if header.len() > 1 {
        (header[0], header[1].trim())
    } else {
        (header[0].trim(), "")
    })
}

/// Split a header into a (name, value) tuple.
/// Drops the header if it isn't valid UTF-8.
pub(super) fn split_or_drop_header(header: &[u8]) -> Option<(&str, &str)> {
    match split_header(header) {
        Ok((name, value)) => {
            log::trace!("Received header: {}: {}", name, value);
            Some((name, value))
        }
        Err(e) => {
            let i = e.valid_up_to();
            log::trace!(
                "Dropping non-UTF-8 header: Valid prefix: {:?}; Invalid bytes: {:x?}",
                str::from_utf8(&header[..i]).unwrap(),
                &header[i..],
            );
            None
        }
    }
}
