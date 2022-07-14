/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;

pub const H2_ALPN: &str = "h2";
pub const HGCLI_ALPN: &str = "hgcli";

pub fn alpn_select<'a>(protos: &'a [u8], desired: &str) -> Result<Option<&'a [u8]>, Error> {
    let mut pos = 0;
    let desired = desired.as_bytes();

    loop {
        let size = match protos.get(pos) {
            Some(size) => size,
            None => return Ok(None),
        };
        let size: usize = (*size).into();

        let end = pos + size;
        if end >= protos.len() {
            return Err(Error::msg("ALPN string is invalid"));
        }

        let slice = &protos[pos + 1..=end];

        if slice == desired {
            return Ok(Some(slice));
        }

        pos = end + 1;
    }
}

pub fn alpn_format(desired: &str) -> Result<Vec<u8>, Error> {
    let desired = desired.as_bytes();
    let mut ret = vec![];
    ret.push(desired.len().try_into().context("ALPN value is too long")?);
    ret.extend(desired);
    Ok(ret)
}

#[cfg(test)]
mod test {
    use super::*;

    use quickcheck::quickcheck;

    #[test]
    pub fn test_alpn_select() -> Result<(), Error> {
        // Valid selections
        assert_eq!(
            alpn_select("\x02h2".as_bytes(), "h2")?,
            Some("h2".as_bytes())
        );
        assert_eq!(
            alpn_select("\x02h2\x05hgcli".as_bytes(), "h2")?,
            Some("h2".as_bytes())
        );
        assert_eq!(
            alpn_select("\x05hgcli\x02h2".as_bytes(), "h2")?,
            Some("h2".as_bytes())
        );
        assert_eq!(alpn_select("\x05hgcli".as_bytes(), "h2")?, None);
        assert_eq!(alpn_select("".as_bytes(), "h2")?, None);

        // Invalid selections
        assert!(alpn_select("\x05hgcl".as_bytes(), "h2").is_err());

        Ok(())
    }

    #[test]
    pub fn test_alpn_format() -> Result<(), Error> {
        assert_eq!(alpn_format("h2")?, "\x02h2".as_bytes());
        assert_eq!(alpn_format("hgcli")?, "\x05hgcli".as_bytes());
        Ok(())
    }

    quickcheck! {
        fn quickcheck_alpn_garbage(bytes: Vec<u8>) -> bool {
            let _ = alpn_select(&bytes, "foo");
            true
        }

        fn quickcheck_alpn_select(protos: Vec<(char, char, char)>) -> bool {
            let protos = protos.into_iter().map(|(c1, c2, c3)| {
                [c1, c2, c3].iter().collect()
            }).collect::<Vec<String>>();

            let req = protos.iter().flat_map(|proto| {
                alpn_format(proto).unwrap().into_iter()
            }).collect::<Vec<_>>();

            for needle in protos.iter() {
                let ret = match alpn_select(&req, needle.as_str()) {
                    Ok(ret) => ret,
                    Err(..) => return false,
                };

                if ret == Some(needle.as_bytes()) {
                    continue;
                }

                return false;
            }

            true
        }
    }
}
