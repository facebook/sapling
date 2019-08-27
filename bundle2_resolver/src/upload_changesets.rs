// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::errors::*;
use crate::stats::*;
use crate::upload_blobs::UploadableHgBlob;
use blobrepo::{BlobRepo, ChangesetHandle, ContentBlobInfo, CreateChangeset};
use context::CoreContext;
use failure::Compat;
use failure_ext::bail_msg;
use failure_ext::StreamFailureErrorExt;
use futures::future::{self, ok, Shared};
use futures::Future;
use futures::{stream, Stream};
use futures_ext::{try_boxfuture, BoxFuture, BoxStream, FutureExt, StreamExt};
use mercurial_revlog::{
    changeset::RevlogChangeset,
    manifest::{Details, ManifestContent},
};
use mercurial_types::{
    blobs::{ChangesetMetadata, HgBlobEntry},
    HgChangesetId, HgManifestId, HgNodeHash, HgNodeKey, MPath, RepoPath, NULL_HASH,
};
use scuba_ext::ScubaSampleBuilder;
use std::collections::HashMap;
use std::ops::AddAssign;
use wirepack::TreemanifestEntry;

type Filelogs = HashMap<HgNodeKey, Shared<BoxFuture<(HgBlobEntry, RepoPath), Compat<Error>>>>;
type ContentBlobs = HashMap<HgNodeKey, ContentBlobInfo>;
type Manifests = HashMap<HgNodeKey, <TreemanifestEntry as UploadableHgBlob>::Value>;
type UploadedChangesets = HashMap<HgChangesetId, ChangesetHandle>;

type HgBlobFuture = BoxFuture<(HgBlobEntry, RepoPath), Error>;
type HgBlobStream = BoxStream<(HgBlobEntry, RepoPath), Error>;

/// In order to generate the DAG of dependencies between Root Manifest and other Manifests and
/// Filelogs we need to walk that DAG.
/// This represents the manifests and file nodes introduced by a particular changeset.
pub struct NewBlobs {
    // root_manifest can be None f.e. when commit removes all the content of the repo
    root_manifest: BoxFuture<Option<(HgBlobEntry, RepoPath)>, Error>,
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
                sub_entries: stream::empty().boxify(),
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
                let entry = (repo.get_root_entry(manifest_root_id), RepoPath::RootPath);
                (vec![], vec![], future::ok(Some(entry)).boxify())
            }
        };

        Ok(Self {
            root_manifest,
            sub_entries: stream::futures_unordered(entries)
                .with_context(move |_| {
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
            bail_msg!(
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
                None => bail_msg!("internal error: joined root path with root manifest"),
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
                        None => {
                            bail_msg!("internal error: content blob future missing for filenode")
                        }
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
) -> impl Future<Item = Option<ChangesetHandle>, Error = Error> {
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

pub fn upload_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    scuba_logger: ScubaSampleBuilder,
    node: HgChangesetId,
    revlog_cs: RevlogChangeset,
    mut uploaded_changesets: UploadedChangesets,
    filelogs: &Filelogs,
    manifests: &Manifests,
    content_blobs: &ContentBlobs,
    draft: bool,
) -> BoxFuture<UploadedChangesets, Error> {
    let (p1, p2) = {
        (
            get_parent(ctx.clone(), &repo, &uploaded_changesets, revlog_cs.p1),
            get_parent(ctx.clone(), &repo, &uploaded_changesets, revlog_cs.p2),
        )
    };
    let NewBlobs {
        root_manifest,
        sub_entries,
        // XXX use these content blobs in the future
        content_blobs: _content_blobs,
    } = try_boxfuture!(NewBlobs::new(
        revlog_cs.manifestid(),
        &manifests,
        &filelogs,
        &content_blobs,
        repo.clone(),
    ));

    // DO NOT replace and_then() with join() or futures_ordered()!
    // It may result in a combinatoral explosion in mergy repos (see D14100259)
    p1.and_then(|p1| p2.map(|p2| (p1, p2)))
        .with_context(move |_| format!("While fetching parents for Changeset {}", node))
        .from_err()
        .and_then(move |(p1, p2)| {
            let cs_metadata = ChangesetMetadata {
                user: String::from_utf8(revlog_cs.user().into())?,
                time: revlog_cs.time().clone(),
                extra: revlog_cs.extra().clone(),
                comments: String::from_utf8(revlog_cs.comments().into())?,
            };
            let create_changeset = CreateChangeset {
                expected_nodeid: Some(node.into_nodehash()),
                expected_files: Some(Vec::from(revlog_cs.files())),
                p1,
                p2,
                root_manifest,
                sub_entries,
                // XXX pass content blobs to CreateChangeset here
                cs_metadata,
                must_check_case_conflicts: true,
                draft,
            };
            let scheduled_uploading = create_changeset.create(ctx, &repo, scuba_logger);

            uploaded_changesets.insert(node, scheduled_uploading);
            Ok(uploaded_changesets)
        })
        .boxify()
}
