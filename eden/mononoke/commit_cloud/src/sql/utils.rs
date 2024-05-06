/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use mercurial_types::sha1_hash::Sha1;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;

#[allow(unused)]
pub fn changeset_from_bytes(bytes: &[u8], is_hex_encoded: bool) -> anyhow::Result<HgChangesetId> {
    if is_hex_encoded {
        return Ok(HgChangesetId::new(HgNodeHash::new(Sha1::from_str(
            std::str::from_utf8(bytes)?,
        )?)));
    }
    HgChangesetId::from_bytes(bytes)
}

#[allow(unused)]
pub fn changeset_as_bytes(cs_id: &HgChangesetId, encode_as_hex: bool) -> anyhow::Result<Vec<u8>> {
    if encode_as_hex {
        let hex = cs_id.to_hex();
        return Ok(hex.as_bytes().to_vec());
    }
    Ok(cs_id.as_bytes().to_vec())
}

pub fn list_as_bytes(
    list: Vec<HgChangesetId>,
    is_hex_encoded: bool,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut res: Vec<Vec<u8>> = Vec::new();
    for cs_id in list {
        res.push(changeset_as_bytes(&cs_id, is_hex_encoded)?);
    }
    Ok(res)
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use mercurial_types::HgChangesetId;
    use once_cell::sync::Lazy;

    use crate::sql::utils::changeset_as_bytes;
    use crate::sql::utils::changeset_from_bytes;

    const HEX_ENCODED: &[u8] = b"2d7d4ba9ce0a6ffd222de7785b249ead9c51c536";

    const BYTES: [u8; 20] = [
        0x2d, 0x7d, 0x4b, 0xa9, 0xce, 0x0a, 0x6f, 0xfd, 0x22, 0x2d, 0xe7, 0x78, 0x5b, 0x24, 0x9e,
        0xad, 0x9c, 0x51, 0xc5, 0x36,
    ];
    static CS_ID: Lazy<HgChangesetId> = Lazy::new(|| {
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")
            .expect("Failed encoding test changeset id")
    });

    #[test]
    fn test_decode_changeset() {
        assert_eq!(changeset_from_bytes(HEX_ENCODED, true).unwrap(), *CS_ID);
        assert_eq!(changeset_from_bytes(&BYTES, false).unwrap(), *CS_ID);
    }

    #[test]
    fn test_encode_changeset() {
        assert_eq!(changeset_as_bytes(&CS_ID, true).unwrap(), HEX_ENCODED);
        assert_eq!(changeset_as_bytes(&CS_ID, false).unwrap(), BYTES);
    }

    #[test]
    fn test_non_decodable() {
        assert!(changeset_from_bytes(HEX_ENCODED, false).is_err());
        assert!(changeset_from_bytes(&BYTES, true).is_err());
        assert!(changeset_from_bytes(&[1, 2, 3, 4], true).is_err());
        assert!(changeset_from_bytes(&[1, 2, 3, 4], false).is_err());
    }
}
