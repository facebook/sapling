/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

pub(crate) fn format_byte_string(value: &impl AsRef<[u8]>, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "b\"")?;
    let bytes = value.as_ref();
    for byte in bytes {
        match byte {
            0..=31 | 127..=255 => write!(f, "\\x{:02x}", byte)?,
            32..=126 => write!(f, "{}", *byte as char)?,
        }
    }
    write!(f, "\"")?;
    Ok(())
}
