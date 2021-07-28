/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! convert.rs - Conversions between Mercurial and Mononoke types.
//!
//! Mercurial and Mononoke use different types to represent similar
//! concepts, such as paths, identifiers, etc. While these types
//! fundamentally represent the same things, they often differ in
//! implementation details, adding some friction when converting.
//!
//! In theory, the conversions should never fail since these types
//! are used to represent the same data on the client and server
//! respectively, so any conversion failure should be considered
//! a bug. Nonetheless, since these types often differ substantially
//! in implentation, it is possible that conversion failures may occur
//! in practice.

use anyhow::{anyhow, bail, Context, Error, Result};
use futures::{stream, StreamExt, TryStreamExt};
use smallvec::SmallVec;
use std::collections::{BTreeMap, HashMap};
use std::str;

use edenapi_types::{
    commit::{BonsaiChangesetContent, BonsaiFileChange},
    token::{UploadTokenData, UploadTokenMetadata},
    AnyFileContentId, AnyId, HgChangesetContent, HgMutationEntryContent,
};
use mercurial_mutation::HgMutationEntry;
use mercurial_types::{
    blobs::Extra, blobs::RevlogChangeset, HgChangesetId, HgManifestId, HgNodeHash,
};
use mononoke_api::path::MononokePath;
use mononoke_api_hg::HgRepoContext;
use mononoke_types::MPath;
use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime, FileChange};
use types::{HgId, RepoPath, RepoPathBuf};

use crate::errors::ErrorKind;

/// Convert a Mercurial `RepoPath` or `RepoPathBuf` into a `MononokePath`.
/// The input will be copied due to differences in data representation.
pub fn to_mononoke_path(path: impl AsRef<RepoPath>) -> Result<MononokePath> {
    Ok(MononokePath::new(to_mpath(path)?))
}

/// Convert a Mercurial `RepoPath` or `RepoPathBuf` into an `Option<MPath>`.
/// The input will be copied due to differences in data representation.
pub fn to_mpath(path: impl AsRef<RepoPath>) -> Result<Option<MPath>> {
    let path_bytes = path.as_ref().as_byte_slice();
    MPath::new_opt(path_bytes).with_context(|| ErrorKind::InvalidPath(path_bytes.to_vec()))
}

/// Convert a `MononokePath` into a Mercurial `RepoPathBuf`.
/// The input will be copied due to differences in data representation.
pub fn to_hg_path(path: &MononokePath) -> Result<RepoPathBuf> {
    let path_bytes = match path.as_mpath() {
        Some(mpath) => mpath.to_vec(),
        None => return Ok(RepoPathBuf::new()),
    };
    RepoPathBuf::from_utf8(path_bytes.clone()).context(ErrorKind::InvalidPath(path_bytes))
}

pub fn to_revlog_changeset(cs: HgChangesetContent) -> Result<RevlogChangeset> {
    Ok(RevlogChangeset {
        p1: cs.parents.p1().cloned().map(HgNodeHash::from),
        p2: cs.parents.p2().cloned().map(HgNodeHash::from),
        manifestid: HgManifestId::new(HgNodeHash::from(cs.manifestid)),
        extra: Extra::new(
            cs.extras
                .into_iter()
                .map(|extra| (extra.key, extra.value))
                .collect::<BTreeMap<_, _>>(),
        ),
        files: cs
            .files
            .into_iter()
            .map(|file| to_mpath(&file)?.context(ErrorKind::UnexpectedEmptyPath))
            .collect::<Result<_, _>>()?,
        message: cs.message,
        time: DateTime::from_timestamp(cs.time, cs.tz)?,
        user: cs.user,
    })
}

fn to_bonsai_file_change(fc: BonsaiFileChange) -> Result<FileChange> {
    // TODO: Verify signature on upload token
    if let UploadTokenData {
        id: AnyId::AnyFileContentId(AnyFileContentId::ContentId(content_id)),
        metadata: Some(UploadTokenMetadata::FileContentTokenMetadata(metadata)),
    } = fc.upload_token.data
    {
        Ok(FileChange::new(
            content_id.into(),
            fc.file_type.into(),
            metadata.content_size,
            None,
        ))
    } else {
        bail!("Invalid upload token format, missing content id or file size metadata.")
    }
}

pub async fn to_bonsai_changeset(
    repo: &HgRepoContext,
    cs: BonsaiChangesetContent,
    hgid_to_csid: &HashMap<HgId, ChangesetId>,
) -> Result<BonsaiChangeset> {
    BonsaiChangesetMut {
        parents: stream::iter(cs.hg_parents)
            .then(|hgid| async move {
                Result::<_, Error>::Ok(match hgid_to_csid.get(&hgid) {
                    Some(csid) => csid.clone(),
                    None => repo
                        .get_bonsai_from_hg(hgid.into())
                        .await?
                        .ok_or_else(|| anyhow!("Parent HgId {} is invalid", hgid))?,
                })
            })
            .try_collect()
            .await?,
        author: cs.author,
        author_date: DateTime::from_timestamp(cs.time, cs.tz)?,
        committer: None,
        committer_date: None,
        message: cs.message,
        extra: cs.extra.into_iter().map(|e| (e.key, e.value)).collect(),
        file_changes: cs
            .file_changes
            .into_iter()
            .map(|(path, fc)| {
                let file_change = to_bonsai_file_change(fc)
                    .with_context(|| anyhow!("Parsing file changes for {}", path))?;
                Ok((
                    to_mpath(path)?.ok_or_else(|| anyhow!("Missing path"))?,
                    Some(file_change),
                ))
            })
            .collect::<Result<_>>()?,
    }
    .freeze()
}

pub fn to_mutation_entry(mutation: HgMutationEntryContent) -> Result<HgMutationEntry> {
    let successor = HgChangesetId::new(HgNodeHash::from(mutation.successor));
    let predecessors = mutation
        .predecessors
        .into_iter()
        .map(HgNodeHash::from)
        .map(HgChangesetId::new)
        .collect::<Vec<_>>();
    let predecessors: SmallVec<[_; 1]> = SmallVec::from_vec(predecessors);
    let split = mutation
        .split
        .into_iter()
        .map(HgNodeHash::from)
        .map(HgChangesetId::new)
        .collect::<Vec<_>>();
    let op = mutation.op;
    let user = str::from_utf8(&mutation.user)?.to_string();
    let time = DateTime::from_timestamp(mutation.time, mutation.tz)?;
    let exta = mutation
        .extras
        .into_iter()
        .map(|extra| {
            Ok((
                str::from_utf8(&extra.key)?.to_string(),
                str::from_utf8(&extra.value)?.to_string(),
            ))
        })
        .collect::<Result<_, Error>>()?;

    Ok(HgMutationEntry::new(
        successor,
        predecessors,
        split,
        op,
        user,
        time,
        exta,
    ))
}
