/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Fallible conversion for Path from byte array
use serde::de;
use std::fmt;
use std::marker::PhantomData;
use std::path::PathBuf;

/// On unix we use https://doc.rust-lang.org/std/os/unix/ffi/trait.OsStrExt.html#tymethod.from_bytes
/// to convert the bytes to an OsStr and then build the Path from that.
/// On Windows, watchman guarantees that paths are represented as UTF-8

#[cfg(unix)]
use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(windows)]
use std::str::from_utf8;

pub fn decode_path_fallible<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct PathBufVisitor(PhantomData<fn() -> PathBuf>);

    impl<'de> de::Visitor<'de> for PathBufVisitor {
        type Value = PathBuf;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("byte array or string or str")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(PathBuf::from(value))
        }

        fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(PathBuf::from(value))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(PathBuf::from(value))
        }

        fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_bytes(&value)
        }

        fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            #[cfg(windows)]
            match from_utf8(value) {
                Err(e) => Err(E::custom(format!("{}", e))),
                Ok(s) => Ok(PathBuf::from(s)),
            }
            #[cfg(unix)]
            Ok(PathBuf::from(OsStr::from_bytes(value)))
        }

        fn visit_borrowed_bytes<E>(self, value: &'de [u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            #[cfg(windows)]
            match from_utf8(&value) {
                Err(e) => Err(E::custom(format!("{}", e))),
                Ok(s) => Ok(PathBuf::from(s)),
            }
            #[cfg(unix)]
            Ok(PathBuf::from(OsStr::from_bytes(value)))
        }
    }
    deserializer.deserialize_any(PathBufVisitor(PhantomData))
}
