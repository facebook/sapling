/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Directory State.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use repolock::LockError;
use repolock::LockedPath;
use repolock::RepoLocker;
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

pub fn flush(
    root: &Path,
    treestate: &mut TreeState,
    locker: &RepoLocker,
    write_time: Option<i64>,
    lock_timeout_secs: Option<u32>,
) -> Result<()> {
    if treestate.dirty() {
        tracing::debug!("flushing dirty treestate");
        let id = identity::must_sniff_dir(root)?;
        let dot_dir = root.join(id.dot_dir());
        let dirstate_path = dot_dir.join("dirstate");

        let _lock = wait_for_wc_lock(dot_dir, locker, lock_timeout_secs)?;

        let dirstate_input = fs_err::read(&dirstate_path)?;
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

        let metadata = treestate.metadata()?;
        dirstate.p1 = metadata
            .get("p1")
            .map_or(Ok(NULL_ID), |p| HgId::from_hex(p.as_bytes()))?;
        dirstate.p2 = metadata
            .get("p2")
            .map_or(Ok(NULL_ID), |p| HgId::from_hex(p.as_bytes()))?;
        let treestate_fields = dirstate.tree_state.as_mut().ok_or_else(|| {
            anyhow!(
                "Unable to flush treestate because dirstate is missing required treestate fields"
            )
        })?;

        let mut dirstate_file = util::file::atomic_open(&dirstate_path)?;

        let write_time = match write_time {
            Some(t) => t,
            None => dirstate_file
                .as_file()
                .metadata()?
                .modified()?
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs()
                .try_into()?,
        };

        // Invalidate entries with mtime >= now so we can notice size preserving
        // edits to files in the same second the dirstate is written (and wlock is released).
        treestate
            .invalidate_mtime(write_time.try_into()?)
            .context("error invalidating dirstate mtime")?;

        let root_id = treestate.flush()?;
        treestate_fields.tree_filename = treestate.file_name()?;
        treestate_fields.tree_root_id = root_id;

        dirstate.serialize(dirstate_file.as_file())?;
        dirstate_file.save()?;

        Ok(())
    } else {
        tracing::debug!("skipping treestate flush - it is not dirty");
        Ok(())
    }
}

pub fn wait_for_wc_lock(
    wc_dot_hg: PathBuf,
    locker: &RepoLocker,
    timeout_secs: Option<u32>,
) -> anyhow::Result<LockedPath> {
    let mut timeout = match timeout_secs {
        None => return Ok(locker.lock_working_copy(wc_dot_hg)?),
        Some(timeout) => timeout,
    };

    loop {
        match locker.try_lock_working_copy(wc_dot_hg.clone()) {
            Ok(lock) => return Ok(lock),
            Err(err) => match err {
                LockError::Contended(_) => {
                    if timeout == 0 {
                        return Err(ErrorKind::LockTimeout.into());
                    }

                    timeout -= 1;

                    std::thread::sleep(Duration::from_secs(1));
                }
                _ => return Err(err.into()),
            },
        }
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
            ds.tree_state.as_mut().unwrap().repack_threshold = Some(123);

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
