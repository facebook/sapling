/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::stats::*;
use crate::upload_blobs::UploadableHgBlob;
use anyhow::{bail, Context, Error, Result};
use blobrepo::{BlobRepo, ChangesetHandle, CreateChangeset};
use context::CoreContext;
use failure_ext::{Compat, StreamFailureErrorExt};
use futures_ext::{
    BoxFuture as OldBoxFuture, BoxStream as OldBoxStream, FutureExt as OldFutureExt,
    StreamExt as OldStreamExt,
};
use futures_old::future::{self as old_future, ok, Shared};
use futures_old::Future as OldFuture;
use futures_old::{stream as old_stream, Stream as OldStream};
use futures_util::compat::Future01CompatExt;
use mercurial_revlog::{
    changeset::RevlogChangeset,
    manifest::{Details, ManifestContent},
};
use mercurial_types::{
    blobs::{ChangesetMetadata, ContentBlobInfo, HgBlobEntry},
    HgChangesetId, HgManifestId, HgNodeHash, HgNodeKey, MPath, RepoPath, NULL_HASH,
};
use scuba_ext::ScubaSampleBuilder;
use std::collections::HashMap;
use std::ops::AddAssign;
use wirepack::TreemanifestEntry;

type Filelogs = HashMap<HgNodeKey, Shared<OldBoxFuture<(HgBlobEntry, RepoPath), Compat<Error>>>>;
type ContentBlobs = HashMap<HgNodeKey, ContentBlobInfo>;
type Manifests = HashMap<HgNodeKey, <TreemanifestEntry as UploadableHgBlob>::Value>;
type UploadedChangesets = HashMap<HgChangesetId, ChangesetHandle>;

type HgBlobFuture = OldBoxFuture<(HgBlobEntry, RepoPath), Error>;
type HgBlobStream = OldBoxStream<(HgBlobEntry, RepoPath), Error>;

/// In order to generate the DAG of dependencies between Root Manifest and other Manifests and
/// Filelogs we need to walk that DAG.
/// This represents the manifests and file nodes introduced by a particular changeset.
pub struct NewBlobs {
    // root_manifest can be None f.e. when commit removes all the content of the repo
    root_manifest: OldBoxFuture<Option<(HgBlobEntry, RepoPath)>, Error>,
    // sub_entries has both submanifest and filenode entries.
    sub_entries: HgBlobStream,
    // This is returned as a Vec rather than a Stream so that the path and metadata are
    // available before the content blob is uploaded. This will allow creating and uploading
    // changeset blobs without being blocked on content blob uploading being complete.
    content_blobs: Vec<ContentBlobInfo>,
}

struct WalkHelperCounters {
    manifests_count: usize,
    filelogs_count: usize,
    content_blobs_count: usize,
}

impl AddAssign for WalkHelperCounters {
    fn add_assign(&mut self, other: WalkHelperCounters) {
        *self = Self {
            manifests_count: self.manifests_count + other.manifests_count,
            filelogs_count: self.filelogs_count + other.filelogs_count,
            content_blobs_count: self.content_blobs_count + other.content_blobs_count,
        };
    }
}

impl NewBlobs {
    pub fn new(
        manifest_root_id: HgManifestId,
        manifests: &Manifests,
        filelogs: &Filelogs,
        content_blobs: &ContentBlobs,
        repo: BlobRepo,
    ) -> Result<Self> {
        if manifest_root_id.into_nodehash() == NULL_HASH {
            // If manifest root id is NULL_HASH then there is no content in this changest
            return Ok(Self {
                root_manifest: ok(None).boxify(),
                sub_entries: old_stream::empty().boxify(),
                content_blobs: Vec::new(),
            });
        }

        let root_key = HgNodeKey {
            path: RepoPath::root(),
            hash: manifest_root_id.clone().into_nodehash(),
        };

        let (entries, content_blobs, root_manifest) = match manifests.get(&root_key) {
            Some((ref manifest_content, ref p1, ref p2, ref manifest_root)) => {
                let (entries, content_blobs, counters) = Self::walk_helper(
                    &RepoPath::root(),
                    &manifest_content,
                    get_manifest_parent_content(manifests, RepoPath::root(), p1.clone()),
                    get_manifest_parent_content(manifests, RepoPath::root(), p2.clone()),
                    manifests,
                    filelogs,
                    content_blobs,
                )?;
                STATS::per_changeset_manifests_count.add_value(counters.manifests_count as i64);
                STATS::per_changeset_filelogs_count.add_value(counters.filelogs_count as i64);
                STATS::per_changeset_content_blobs_count
                    .add_value(counters.content_blobs_count as i64);
                let root_manifest = manifest_root
                    .clone()
                    .map(|it| Some((*it).clone()))
                    .from_err()
                    .boxify();

                (entries, content_blobs, root_manifest)
            }
            None => {
                let entry = (
                    HgBlobEntry::new_root(repo.blobstore().boxed(), manifest_root_id),
                    RepoPath::RootPath,
                );
                (vec![], vec![], old_future::ok(Some(entry)).boxify())
            }
        };

        Ok(Self {
            root_manifest,
            sub_entries: old_stream::futures_unordered(entries)
                .with_context(move || {
                    format!(
                        "While walking dependencies of Root Manifest with id {:?}",
                        manifest_root_id
                    )
                })
                .from_err()
                .boxify(),
            content_blobs,
        })
    }

    fn walk_helper(
        path_taken: &RepoPath,
        manifest_content: &ManifestContent,
        p1: Option<&ManifestContent>,
        p2: Option<&ManifestContent>,
        manifests: &Manifests,
        filelogs: &Filelogs,
        content_blobs: &ContentBlobs,
    ) -> Result<(Vec<HgBlobFuture>, Vec<ContentBlobInfo>, WalkHelperCounters)> {
        if path_taken.len() > 4096 {
            bail!(
                "Exceeded max manifest path during walking with path: {:?}",
                path_taken
            );
        }

        let mut entries: Vec<HgBlobFuture> = Vec::new();
        let mut cbinfos: Vec<ContentBlobInfo> = Vec::new();
        let mut counters = WalkHelperCounters {
            manifests_count: 0,
            filelogs_count: 0,
            content_blobs_count: 0,
        };

        for (name, details) in manifest_content.files.iter() {
            if is_entry_present_in_parent(p1, name, details)
                || is_entry_present_in_parent(p2, name, details)
            {
                // If one of the parents contains exactly the same version of entry then either that
                // file or manifest subtree is not new
                continue;
            }

            let nodehash = details.entryid().clone().into_nodehash();
            let next_path = MPath::join_opt(path_taken.mpath(), name);
            let next_path = match next_path {
                Some(path) => path,
                None => bail!("internal error: joined root path with root manifest"),
            };

            if details.is_tree() {
                let key = HgNodeKey {
                    path: RepoPath::DirectoryPath(next_path),
                    hash: nodehash,
                };

                if let Some(&(ref manifest_content, ref p1, ref p2, ref blobfuture)) =
                    manifests.get(&key)
                {
                    counters.manifests_count += 1;
                    entries.push(
                        blobfuture
                            .clone()
                            .map(|it| (*it).clone())
                            .from_err()
                            .boxify(),
                    );
                    let (mut walked_entries, mut walked_cbinfos, sub_counters) = Self::walk_helper(
                        &key.path,
                        manifest_content,
                        get_manifest_parent_content(manifests, key.path.clone(), p1.clone()),
                        get_manifest_parent_content(manifests, key.path.clone(), p2.clone()),
                        manifests,
                        filelogs,
                        content_blobs,
                    )?;
                    entries.append(&mut walked_entries);
                    cbinfos.append(&mut walked_cbinfos);
                    counters += sub_counters;
                }
            } else {
                let key = HgNodeKey {
                    path: RepoPath::FilePath(next_path),
                    hash: nodehash,
                };
                if let Some(blobfuture) = filelogs.get(&key) {
                    counters.filelogs_count += 1;
                    counters.content_blobs_count += 1;
                    entries.push(
                        blobfuture
                            .clone()
                            .map(|it| (*it).clone())
                            .from_err()
                            .boxify(),
                    );
                    match content_blobs.get(&key) {
                        Some(cbinfo) => cbinfos.push(cbinfo.clone()),
                        None => bail!("internal error: content blob future missing for filenode"),
                    }
                }
            }
        }

        Ok((entries, cbinfos, counters))
    }
}

fn get_manifest_parent_content(
    manifests: &Manifests,
    path: RepoPath,
    p: Option<HgNodeHash>,
) -> Option<&ManifestContent> {
    p.and_then(|p| manifests.get(&HgNodeKey { path, hash: p }))
        .map(|&(ref content, ..)| content)
}

fn is_entry_present_in_parent(
    p: Option<&ManifestContent>,
    name: &MPath,
    details: &Details,
) -> bool {
    match p.and_then(|p| p.files.get(name)) {
        None => false,
        Some(parent_details) => parent_details == details,
    }
}

/// Retrieves the parent from uploaded changesets, if it is missing then fetches it from BlobRepo
fn get_parent(
    ctx: CoreContext,
    repo: &BlobRepo,
    map: &UploadedChangesets,
    p: Option<HgNodeHash>,
) -> impl OldFuture<Item = Option<ChangesetHandle>, Error = Error> {
    let res = match p {
        None => None,
        Some(p) => match map.get(&HgChangesetId::new(p)) {
            None => Some(ChangesetHandle::ready_cs_handle(
                ctx,
                repo.clone(),
                HgChangesetId::new(p),
            )),
            Some(cs) => Some(cs.clone()),
        },
    };
    ok(res)
}

pub async fn upload_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    scuba_logger: ScubaSampleBuilder,
    node: HgChangesetId,
    revlog_cs: &RevlogChangeset,
    mut uploaded_changesets: UploadedChangesets,
    filelogs: &Filelogs,
    manifests: &Manifests,
    content_blobs: &ContentBlobs,
    must_check_case_conflicts: bool,
) -> Result<UploadedChangesets, Error> {
    let NewBlobs {
        root_manifest,
        sub_entries,
        // XXX use these content blobs in the future
        content_blobs: _content_blobs,
    } = NewBlobs::new(
        revlog_cs.manifestid(),
        &manifests,
        &filelogs,
        &content_blobs,
        repo.clone(),
    )?;

    let cs_metadata = ChangesetMetadata {
        user: String::from_utf8(revlog_cs.user().into())?,
        time: revlog_cs.time().clone(),
        extra: revlog_cs.extra().clone(),
        comments: String::from_utf8(revlog_cs.comments().into())?,
    };

    // DO NOT try to comute p1 and p2 concurrently!
    // It may result in a combinatoral explosion in mergy repos (see D14100259)
    let p1 = get_parent(ctx.clone(), &repo, &uploaded_changesets, revlog_cs.p1)
        .boxify()
        .compat()
        .await
        .with_context(move || format!("While fetching parents for Changeset {}", node))?;

    let p2 = get_parent(ctx.clone(), &repo, &uploaded_changesets, revlog_cs.p2)
        .boxify()
        .compat()
        .await
        .with_context(move || format!("While fetching parents for Changeset {}", node))?;

    let create_changeset = CreateChangeset {
        expected_nodeid: Some(node.into_nodehash()),
        expected_files: Some(Vec::from(revlog_cs.files())),
        p1,
        p2,
        root_manifest,
        sub_entries,
        // XXX pass content blobs to CreateChangeset here
        cs_metadata,
        must_check_case_conflicts,
    };
    let scheduled_uploading = create_changeset.create(ctx, &repo, scuba_logger);

    uploaded_changesets.insert(node, scheduled_uploading);
    Ok(uploaded_changesets)
}
