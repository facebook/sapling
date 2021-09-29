/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

macro_rules! sized_hash {
    ($name: ident, $size: literal) => {
        #[derive(
            Clone,
            Copy,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde_derive::Serialize,
            serde_derive::Deserialize
        )]
        pub struct $name([u8; $name::len()]);

        impl $name {
            pub const fn len() -> usize {
                $size
            }

            pub const fn len_hex() -> usize {
                Self::len() * 2
            }
        }

        impl From<[u8; $name::len()]> for $name {
            fn from(v: [u8; $name::len()]) -> Self {
                $name(v)
            }
        }

        impl From<$name> for [u8; $name::len()] {
            fn from(v: $name) -> Self {
                v.0
            }
        }

        impl AsRef<[u8]> for $name {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                for d in &self.0 {
                    write!(fmt, "{:02x}", d)?;
                }
                Ok(())
            }
        }

        impl ::std::fmt::Debug for $name {
            fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                write!(fmt, "{}(\"{}\")", stringify!($name), self)
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::ServerError;

            fn from_str(s: &str) -> Result<$name, Self::Err> {
                if s.len() != $name::len_hex() {
                    return Err(Self::Err::generic(format!(
                        "{} parsing failure: need exactly {} hex digits",
                        stringify!(name),
                        $name::len_hex()
                    )));
                }
                let mut ret = $name([0; $name::len()]);
                match faster_hex::hex_decode(s.as_bytes(), &mut ret.0) {
                    Ok(_) => Ok(ret),
                    Err(_) => Err(Self::Err::generic(concat!(
                        stringify!($name),
                        " parsing failure: bad hex character"
                    ))),
                }
            }
        }

        #[cfg(any(test, feature = "for-tests"))]
        impl quickcheck::Arbitrary for $name {
            fn arbitrary(g: &mut quickcheck::Gen) -> Self {
                let mut v = Self::default();
                for b in v.0.iter_mut() {
                    *b = u8::arbitrary(g);
                }
                v
            }
        }
    };
}

macro_rules! blake2_hash {
    ($name: ident) => {
        sized_hash!($name, 32);
    };
}
