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
            b'\\' | b'"' => write!(f, "\\{}", *byte as char)?,
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
///
/// ```
pub(crate) fn format_byte(byte: &u8, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "b'")?;
    match byte {
        b'\\' | b'\'' => write!(f, "\\{}", *byte as char)?,
        0..=31 | 127..=255 => write!(f, "\\x{:02x}", byte)?,
        32..=126 => write!(f, "{}", *byte as char)?,
    }
    write!(f, "'")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_format_byte_string() {
        struct FormatByteString(&'static [u8]);
        impl fmt::Display for FormatByteString {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                format_byte_string(&self.0, f)
            }
        }
        assert_eq!(
            FormatByteString(&[0, 10, 31, 32, 34, 39, 109, 126, 127, 255])
                .to_string()
                .as_str(),
            r#"b"\x00\x0a\x1f \"'m~\x7f\xff""#
        );
    }

    #[mononoke::test]
    fn test_format_byte() {
        struct FormatByte(u8);
        impl fmt::Display for FormatByte {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                format_byte(&self.0, f)
            }
        }
        let test_eq = |byte, output| {
            assert_eq!(FormatByte(byte).to_string().as_str(), output);
        };

        test_eq(0, r"b'\x00'");
        test_eq(10, r"b'\x0a'");
        test_eq(31, r"b'\x1f'");
        test_eq(32, r"b' '");
        test_eq(34, r#"b'"'"#);
        test_eq(39, r"b'\''");
        test_eq(109, r"b'm'");
        test_eq(126, r"b'~'");
        test_eq(127, r"b'\x7f'");
        test_eq(255, r"b'\xff'");
    }
}
