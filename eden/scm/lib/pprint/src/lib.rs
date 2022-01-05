/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # `pprint`
//!
//! Pretty-print a serde value.
//!
//! - Support non-utf8 bytes.
//! - Support non-string keys in maps.
//! - Auto convert 20 or 32-byte hashes to hex format.
//! - Output is valid Python syntax.

use serde::Serialize;
pub use serde_cbor::Value;

/// Pretty-print a serializable `value` to a string.
pub fn pformat<T: Serialize>(value: &T) -> serde_cbor::Result<String> {
    let value = serde_cbor::value::to_value(value)?;
    Ok(pformat_value(&value))
}

/// Pretty-print `value` to a string.
pub fn pformat_value(value: &Value) -> String {
    let mut out = String::new();
    format_value(value, 0, &mut out);
    out
}

/// Print byte sequence with escapes.
fn format_bytes(value: &[u8], out: &mut String) {
    out.push_str("b\"");
    for &b in value {
        match b {
            0 => out.push_str("\\0"),
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            _ => {
                if b >= b' ' && b < 0x7f {
                    out.push(b as char)
                } else {
                    out.push_str("\\x");
                    out.push_str(&to_hex(&[b]));
                }
            }
        }
    }
    out.push('"');
}

/// Main entry. Write `value` to `out`.
/// For added new lines, prefix them with `indent` spaces.
fn format_value(value: &Value, indent: usize, out: &mut String) {
    use Value::Array;
    use Value::Bool;
    use Value::Bytes;
    use Value::Float;
    use Value::Integer;
    use Value::Map;
    use Value::Null;
    use Value::Text;
    match value {
        Null => out.push_str("None"),
        Bool(v) => out.push_str(if *v { "True" } else { "False" }),
        Integer(v) => out.push_str(&format!("{}", v)),
        Float(v) => out.push_str(&format!("{}", v)),
        Bytes(v) => {
            if [20, 32].contains(&v.len()) {
                out.push_str(&format!("bin({:?})", to_hex(v)));
            } else {
                format_bytes(&v, out);
            }
        }
        Text(v) => out.push_str(&format!("{:?}", v)),
        Array(a) => {
            out.push_str("[");
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                    out.push_str(&" ".repeat(indent + 1));
                }
                format_value(v, indent + 1, out);
                if i + 1 < a.len() {
                    out.push(',');
                }
            }
            out.push(']');
        }
        Map(m) => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                    out.push_str(&" ".repeat(indent + 1));
                }
                let kfmt = pformat_value(k);
                out.push_str(&kfmt);
                out.push_str(": ");
                format_value(v, indent + kfmt.len() + 3, out);
                if i + 1 < m.len() {
                    out.push(',');
                }
            }
            out.push('}')
        }
        _ => {}
    }
}

fn to_hex(slice: &[u8]) -> String {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut v = Vec::with_capacity(slice.len() * 2);
    for &byte in slice {
        v.push(HEX_CHARS[(byte >> 4) as usize]);
        v.push(HEX_CHARS[(byte & 0xf) as usize]);
    }
    unsafe { String::from_utf8_unchecked(v) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: &impl Serialize) -> String {
        pformat(v).unwrap()
    }

    #[test]
    fn test_pformat() {
        assert_eq!(p(&[1, 2]), "[1,\n 2]");
        assert_eq!(p(&["a\n", "b"]), "[\"a\\n\",\n \"b\"]");
        assert_eq!(
            p(&[
                Value::Bytes(b"\0\x01".to_vec()),
                Value::Bytes(b"3\n4\t5".to_vec())
            ]),
            "[b\"\\0\\x01\",\n b\"3\\n4\\t5\"]"
        );
        assert_eq!(p(&[true, false]), "[True,\n False]");
        assert_eq!(p(&[Some(33), None]), "[33,\n None]");

        use std::collections::HashMap;
        let mut m = HashMap::new();
        m.insert(10, vec![2, 3]);
        m.insert(5, vec![4, 5]);
        assert_eq!(p(&m), "{5: [4,\n     5],\n 10: [2,\n      3]}");
    }

    #[test]
    fn test_hex_quote() {
        let v = Value::Bytes(b"12345678901234567890".to_vec());
        assert_eq!(
            p(&[&v]),
            "[bin(\"3132333435363738393031323334353637383930\")]"
        );
    }
}
