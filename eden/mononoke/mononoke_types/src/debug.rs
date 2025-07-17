/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

/// Format a byte array as a byte string literal with non-printable
/// characters escaped.
///
/// For example `[79, 107, 10]` is shown as `b"Ok\x0a"`.
pub(crate) fn format_byte_string(value: &impl AsRef<[u8]>, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "b\"")?;
    let bytes = value.as_ref();
    for byte in bytes {
        match byte {
            b'\\' => write!(f, "\\\\")?,
            0..=31 | 127..=255 => write!(f, "\\x{:02x}", byte)?,
            32..=126 => write!(f, "{}", *byte as char)?,
        }
    }
    write!(f, "\"")?;
    Ok(())
}

/// Format a byte as a byte literal with non-printable characters escaped.
///
/// For example, `113` is shown as `b'q'` and `8` is shown as `b'\x08'`.
pub(crate) fn format_byte(byte: &u8, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "b'")?;
    match byte {
        b'\\' => write!(f, "\\\\")?,
        0..=31 | 127..=255 => write!(f, "\\x{:02x}", byte)?,
        32..=126 => write!(f, "{}", *byte as char)?,
    }
    write!(f, "'")?;
    Ok(())
}
