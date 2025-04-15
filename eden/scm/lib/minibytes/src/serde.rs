/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cell::RefCell;
use std::fmt;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de;

use crate::Bytes;
use crate::Text;

impl Serialize for Bytes {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self)
    }
}

impl Serialize for Text {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self)
    }
}

struct BytesVisitor;
struct TextVisitor;

thread_local! {
    static DESERIALIZE_HINT: RefCell<Option<Bytes>> = const { RefCell::new(None) };
}

fn set_deserialize_hint(bytes: Option<Bytes>) -> Option<Bytes> {
    DESERIALIZE_HINT.with_borrow_mut(|f| {
        let orig = f.take();
        *f = bytes;
        orig
    })
}

impl Bytes {
    /// Call `func` with a "deserialize hint" as an attempt to avoid `memcpy`s.
    /// `func` is usually a serde deserialize function taking `self` as input.
    ///
    /// Only affects the current thread, with the assumption that serde
    /// deserialize is usually single threaded.
    pub fn as_deserialize_hint<R>(&self, func: impl Fn() -> R) -> R {
        let orig = set_deserialize_hint(Some(self.clone()));
        let result = func();
        set_deserialize_hint(orig);
        result
    }
}

impl<'de> de::Visitor<'de> for BytesVisitor {
    type Value = Bytes;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("byte slice")
    }

    fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
        self.visit_bytes(v)
    }

    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(Bytes::copy_from_slice(v.as_bytes()))
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        Ok(Bytes::from(v))
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E> {
        let bytes = DESERIALIZE_HINT.with_borrow(|parent_buffer| match parent_buffer {
            Some(buf) => buf.slice_to_bytes(v),
            None => Bytes::copy_from_slice(v),
        });
        Ok(bytes)
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_byte_buf(BytesVisitor)
    }
}

impl<'de> de::Visitor<'de> for TextVisitor {
    type Value = Text;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("str")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        let text = DESERIALIZE_HINT.with_borrow(|parent_buffer| match parent_buffer {
            Some(buf) => {
                let buf = buf.slice_to_bytes(v.as_bytes());
                // safety: `v: &str` proves `v` and therefore `buf` valid utf-8.
                unsafe { Text::from_utf8_unchecked(buf) }
            }
            None => Text::copy_from_slice(v),
        });
        Ok(text)
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(Text::from(v))
    }
}

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_string(TextVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Serialize)]
    struct S {
        a: Bytes,
        b: Bytes,
        c: Text,
    }

    #[test]
    fn test_deserialize_hint() {
        let s1 = S {
            a: Bytes::copy_from_slice(b"aaaa"),
            b: Bytes::from_static(b"bbbb"),
            c: Text::from_static("ccc"),
        };
        let serialized = Bytes::from(serde_cbor::to_vec(&s1).unwrap());

        // Deserialize directly - no zero copy.
        let s2: S = serde_cbor::from_slice(&serialized).unwrap();
        assert!(serialized.range_of_slice(s2.a.as_ref()).is_none());
        assert!(serialized.range_of_slice(s2.b.as_ref()).is_none());
        assert!(serialized.range_of_slice(s2.c.as_bytes()).is_none());

        // Deserialize with hint - can be zero copy.
        let s3: S = serialized.as_deserialize_hint(|| serde_cbor::from_slice(&serialized).unwrap());
        assert!(serialized.range_of_slice(s3.a.as_ref()).is_some());
        assert!(serialized.range_of_slice(s3.b.as_ref()).is_some());
        assert!(serialized.range_of_slice(s3.c.as_bytes()).is_some());
    }
}
