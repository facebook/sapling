/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Directory State.

use std::io::Write;
use std::path::Path;

use anyhow::anyhow;
use anyhow::Result;
use configmodel::Config;
use types::hgid::NULL_ID;
use types::HgId;

use crate::serialization::Serializable;
use crate::store::BlockId;
use crate::treestate::TreeState;
use crate::ErrorKind;

/// A dirstate object. This maintains .hg/dirstate file
#[derive(Debug, PartialEq)]
pub struct Dirstate {
    pub p1: HgId,
    pub p2: HgId,

    pub tree_state: Option<TreeStateFields>,
}

#[derive(Debug, PartialEq)]
pub struct TreeStateFields {
    // Final component of treestate file. Normally a UUID.
    pub tree_filename: String,
    pub tree_root_id: BlockId,
    pub repack_threshold: Option<u64>,
}

pub fn flush(config: &dyn Config, root: &Path, treestate: &mut TreeState) -> Result<()> {
    if treestate.dirty() {
        tracing::debug!("flushing dirty treestate");
        let id = identity::must_sniff_dir(root)?;
        let dot_dir = root.join(id.dot_dir());
        let dirstate_path = dot_dir.join("dirstate");

        let _locked = repolock::lock_working_copy(&config, &dot_dir)?;

        let dirstate_input = util::file::read(&dirstate_path)?;
        let mut dirstate = Dirstate::deserialize(&mut dirstate_input.as_slice())?;

        // If the dirstate has changed since we last loaded it, don't flush since we might
        // overwrite data. For instance, if we start running 'hg status', it loads the dirstate and
        // treestate and starts updating the treestate.  Before status gets to this flush, if
        // another process, like 'hg checkout' writes to the dirstate/treestate, then if we let
        // this 'hg status' flush it's old data, we'd result in a dirty working copy where the
        // clean checkout data was thought to be dirty because we had old treestate data.
        //
        // In that situation, just return an error and the client can decide if that's ok or not.

        if let Some(dirstate_fields) = &dirstate.tree_state {
            if treestate.file_name()? != dirstate_fields.tree_filename
                || treestate.original_root_id() != dirstate_fields.tree_root_id
            {
                return Err(ErrorKind::TreestateOutOfDate.into());
            }
        }

        dirstate.p1 = treestate
            .get_metadata_by_key("p1")?
            .map_or(Ok(NULL_ID), |p| HgId::from_hex(p.as_bytes()))?;
        dirstate.p2 = treestate
            .get_metadata_by_key("p2")?
            .map_or(Ok(NULL_ID), |p| HgId::from_hex(p.as_bytes()))?;
        let treestate_fields = dirstate.tree_state.as_mut().ok_or_else(|| {
            anyhow!(
                "Unable to flush treestate because dirstate is missing required treestate fields"
            )
        })?;

        let root_id = treestate.flush()?;
        treestate_fields.tree_filename = treestate.file_name()?;
        treestate_fields.tree_root_id = root_id;

        let mut dirstate_output: Vec<u8> = Vec::new();
        dirstate.serialize(&mut dirstate_output).unwrap();
        util::file::atomic_write(&dirstate_path, |file| file.write_all(&dirstate_output))
            .map_err(|e| anyhow!(e))
            .map(|_| ())
    } else {
        tracing::debug!("skipping treestate flush - it is not dirty");
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use types::hgid::NULL_ID;

    use super::*;
    use crate::serialization::Serializable;

    #[test]
    fn test_serialization() -> anyhow::Result<()> {
        let mut ds = Dirstate {
            p1: HgId::from_hex(b"93a7f768ac7506e31015dfa545b7f1475a76c4cf")?,
            p2: NULL_ID,
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
