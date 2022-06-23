/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Directory State.

use types::HgId;

use crate::store::BlockId;

/// A dirstate object. This maintains .hg/dirstate file
#[derive(Debug, PartialEq)]
pub struct Dirstate {
    pub p0: HgId,
    pub p1: HgId,

    pub tree_state: Option<TreeStateFields>,
}

#[derive(Debug, PartialEq)]
pub struct TreeStateFields {
    // Final component of treestate file. Normally a UUID.
    pub tree_filename: String,
    pub tree_root_id: BlockId,
    pub repack_threshold: Option<u64>,
}

#[cfg(test)]
mod test {
    use types::hgid::NULL_ID;

    use super::*;
    use crate::serialization::Serializable;

    #[test]
    fn test_serialization() -> anyhow::Result<()> {
        let mut ds = Dirstate {
            p0: HgId::from_hex(b"93a7f768ac7506e31015dfa545b7f1475a76c4cf")?,
            p1: NULL_ID,
            tree_state: Some(TreeStateFields {
                tree_filename: "2c715852-5e8c-45bf-b1f2-236e25dd648b".to_string(),
                tree_root_id: BlockId(2236480),
                repack_threshold: None,
            }),
        };

        {
            let mut buf: Vec<u8> = Vec::new();
            ds.serialize(&mut buf).unwrap();
            assert_eq!(
                &buf,
                // I pulled this out of a real sparse .hg/dirstate.
                b"\x93\xa7\xf7\x68\xac\x75\x06\xe3\x10\x15\xdf\xa5\x45\xb7\xf1\x47\x5a\x76\xc4\xcf\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\ntreestate\n\0filename=2c715852-5e8c-45bf-b1f2-236e25dd648b\0rootid=2236480",
            );

            let got = Dirstate::deserialize(&mut buf.as_slice())?;
            assert_eq!(got, ds);
        }

        {
            (&mut ds.tree_state).as_mut().unwrap().repack_threshold = Some(123);

            let mut buf: Vec<u8> = Vec::new();
            ds.serialize(&mut buf).unwrap();
            assert_eq!(
                &buf,
                b"\x93\xa7\xf7\x68\xac\x75\x06\xe3\x10\x15\xdf\xa5\x45\xb7\xf1\x47\x5a\x76\xc4\xcf\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\ntreestate\n\0filename=2c715852-5e8c-45bf-b1f2-236e25dd648b\0rootid=2236480\0threshold=123",
            );

            let got = Dirstate::deserialize(&mut buf.as_slice())?;
            assert_eq!(got, ds);
        }

        Ok(())
    }
}
