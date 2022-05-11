/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

#[derive(Debug, PartialEq)]
pub struct Metadata(pub(crate) BTreeMap<String, String>);

#[cfg(test)]
mod test {
    use super::*;
    use crate::serialization::Serializable;

    #[test]
    fn test_serialization() -> anyhow::Result<()> {
        let roundtrip = |data: Vec<(&str, &str)>, exp: &[u8]| -> anyhow::Result<()> {
            let meta = Metadata(
                data.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            );
            let mut buf: Vec<u8> = Vec::new();
            meta.serialize(&mut buf)?;
            assert_eq!(buf, exp);
            assert_eq!(Metadata::deserialize(&mut &buf[..])?, meta);

            Ok(())
        };

        roundtrip(Vec::new(), b"")?;
        roundtrip(vec![("foo", "bar")], b"foo=bar")?;
        roundtrip(vec![("abc", "123"), ("xyz", "456")], b"abc=123\0xyz=456")?;
        roundtrip(
            vec![("abc", "why=dothis"), ("你", "好")],
            b"abc=why=dothis\0\xE4\xBD\xA0=\xE5\xA5\xBD",
        )?;

        Ok(())
    }

    #[test]
    fn test_serializate_empty_value() -> anyhow::Result<()> {
        let meta = Metadata(BTreeMap::from([
            ("foo".to_string(), "bar".to_string()),
            ("empty".to_string(), "".to_string()),
        ]));
        let mut buf: Vec<u8> = Vec::new();
        meta.serialize(&mut buf)?;
        assert_eq!(buf, b"foo=bar");

        Ok(())
    }

    #[test]
    fn test_serialize_errors() {
        {
            let meta = Metadata(BTreeMap::from([("foo=bar".to_string(), "baz".to_string())]));
            let mut buf: Vec<u8> = Vec::new();
            assert!(meta.serialize(&mut buf).is_err());
        }

        {
            let meta = Metadata(BTreeMap::from([(
                "foo".to_string(),
                "baz\0qux".to_string(),
            )]));
            let mut buf: Vec<u8> = Vec::new();
            assert!(meta.serialize(&mut buf).is_err());
        }

        {
            let meta = Metadata(BTreeMap::from([(
                "foo\0oops".to_string(),
                "bar".to_string(),
            )]));
            let mut buf: Vec<u8> = Vec::new();
            assert!(meta.serialize(&mut buf).is_err());
        }
    }

    #[test]
    fn test_deserialize_errors() {
        assert!(Metadata::deserialize(&mut &b"foo"[..]).is_err());
        assert!(Metadata::deserialize(&mut &b"foo=bar\0baz"[..]).is_err());
        assert!(Metadata::deserialize(&mut &b"\0"[..]).is_err());
    }
}
