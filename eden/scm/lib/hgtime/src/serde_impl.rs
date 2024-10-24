/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use serde::de;
use serde::de::Error;
use serde::de::IgnoredAny;
use serde::de::Unexpected;
use serde::ser::SerializeTuple;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

use crate::HgTime;

// serialize as a tuple of 2 integers: (time, offset).
impl Serialize for HgTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut t = serializer.serialize_tuple(2)?;
        t.serialize_element(&self.unixtime)?;
        t.serialize_element(&self.offset)?;
        t.end()
    }
}

// deserialize from either:
// - (time: int, offset: int): serialize format
// - 'time offset': str, parse and deserialize
struct HgTimeVisitor;

impl<'de> de::Visitor<'de> for HgTimeVisitor {
    type Value = HgTime;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("(time, offset) tuple or string")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        // space separated int tuple
        if let Some((unixtime_str, offset_str)) = v.split_once(' ') {
            if let (Ok(unixtime), Ok(offset)) =
                (unixtime_str.parse::<i64>(), offset_str.parse::<i32>())
            {
                return Ok(HgTime { unixtime, offset });
            }
        }
        // date str
        match HgTime::parse(v) {
            Some(v) => Ok(v),
            None => Err(E::invalid_value(Unexpected::Str(v), &"HgTime str")),
        }
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let unixtime: i64 = seq
            .next_element()?
            .ok_or_else(|| A::Error::missing_field("unixtime"))?;
        let offset: i32 = seq
            .next_element()?
            .ok_or_else(|| A::Error::missing_field("offset"))?;
        if let Some(remaining) = seq.size_hint() {
            if remaining > 0 {
                return Err(A::Error::invalid_length(2 + remaining, &"2"));
            }
        } else {
            // No concrete size.
            let next: Option<IgnoredAny> = seq.next_element()?;
            if next.is_some() {
                return Err(A::Error::invalid_length(3, &"2"));
            }
        }
        Ok(HgTime { unixtime, offset })
    }
}

impl<'de> Deserialize<'de> for HgTime {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(HgTimeVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_round_trip(t: HgTime) {
        let s = serde_cbor::to_vec(&t).unwrap();
        let t2 = serde_cbor::from_slice(&s).unwrap();
        assert_eq!(t, t2);
        let (unixtime, offset): (i64, i32) = serde_cbor::from_slice(&s).unwrap();
        assert_eq!(t.unixtime, unixtime);
        assert_eq!(t.offset, offset);
    }

    #[test]
    fn test_basic_round_trip() {
        for unixtime in [i64::MIN, -1, 0, 1, i64::MAX] {
            for offset in [i32::MIN, -1, 0, 1, i32::MAX] {
                check_round_trip(HgTime { unixtime, offset });
            }
        }
    }

    fn deserialize_from(v: impl Serialize) -> String {
        let s = serde_cbor::to_vec(&v).unwrap();
        match serde_cbor::from_slice::<HgTime>(&s) {
            Err(e) => format!("Err({})", e),
            Ok(v) => format!("{} {}", v.unixtime, v.offset),
        }
    }

    #[test]
    fn test_deserialize_from_custom_types() {
        // sequences
        assert_eq!(deserialize_from((12, 34)), "12 34");
        assert_eq!(deserialize_from([12, 34]), "12 34");
        assert_eq!(
            deserialize_from((12, 34, 56)),
            "Err(invalid length 3, expected 2)"
        );
        assert_eq!(deserialize_from((12,)), "Err(missing field `offset`)");

        // strings
        assert_eq!(deserialize_from("-11 -22"), "-11 -22");
        assert_eq!(deserialize_from("2000-1-1 +0800"), "946656000 -28800");
        assert_eq!(
            deserialize_from("guess what"),
            "Err(invalid value: string \"guess what\", expected HgTime str)"
        );

        // other types
        assert_eq!(
            deserialize_from(()),
            "Err(invalid type: null, expected (time, offset) tuple or string)"
        );
    }
}
