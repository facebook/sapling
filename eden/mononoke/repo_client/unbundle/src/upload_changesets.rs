/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::manifest::Entry;
use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use blobrepo_hg::create_bonsai_changeset_hook;
use blobrepo_hg::ChangesetHandle;
use blobrepo_hg::CreateChangeset;
use context::CoreContext;
use futures::future;
use futures::future::BoxFuture;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::FuturesUnordered;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use mercurial_revlog::changeset::RevlogChangeset;
use mercurial_revlog::manifest::Details;
use mercurial_revlog::manifest::ManifestContent;
use mercurial_types::blobs::ChangesetMetadata;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgNodeKey;
use mercurial_types::MPath;
use mercurial_types::RepoPath;
use mercurial_types::NULL_HASH;
use scuba_ext::MononokeScubaSampleBuilder;
use std::collections::HashMap;
use std::ops::AddAssign;
use wirepack::TreemanifestEntry;

use crate::changegroup::Filelog;
use crate::stats::*;
use crate::upload_blobs::UploadableHgBlob;

pub type Filelogs = HashMap<HgNodeKey, <Filelog as UploadableHgBlob>::Value>;
pub type Manifests = HashMap<HgNodeKey, <TreemanifestEntry as UploadableHgBlob>::Value>;
pub type UploadedChangesets = HashMap<HgChangesetId, ChangesetHandle>;

type HgBlobFuture = BoxFuture<'static, Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath)>>;
type HgBlobStream = BoxStream<'static, Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath)>>;

/// In order to generate the DAG of dependencies between Root Manifest and other Manifests and
/// Filelogs we need to walk that DAG.
/// This represents the manifests and file nodes introduced by a particular changeset.
pub(crate) struct NewBlobs {
    // root_manifest can be None f.e. when commit removes all the content of the repo
    root_manifest: BoxFuture<'static, Result<Option<(HgManifestId, RepoPath)>>>,
    // sub_entries has both submanifest and filenode entries.
    sub_entries: HgBlobStream,
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
    pub(crate) fn new(
        manifest_root_id: HgManifestId,
        manifests: &Manifests,
        filelogs: &Filelogs,
    ) -> Result<Self> {
        if manifest_root_id.into_nodehash() == NULL_HASH {
            // If manifest root id is NULL_HASH then there is no content in this changest
            return Ok(Self {
                root_manifest: future::ok(None).boxed(),
                sub_entries: stream::empty().boxed(),
            });
        }

        let root_key = HgNodeKey {
            path: RepoPath::root(),
            hash: manifest_root_id.clone().into_nodehash(),
        };

        let (entries, root_manifest) = match manifests.get(&root_key) {
            Some((ref manifest_content, ref p1, ref p2, ref manifest_root)) => {
                let (entries, counters) = Self::walk_helper(
                    &RepoPath::root(),
                    manifest_content,
                    get_manifest_parent_content(manifests, RepoPath::root(), p1.clone()),
                    get_manifest_parent_content(manifests, RepoPath::root(), p2.clone()),
                    manifests,
                    filelogs,
                )?;
                STATS::per_changeset_manifests_count.add_value(counters.manifests_count as i64);
                STATS::per_changeset_filelogs_count.add_value(counters.filelogs_count as i64);
                STATS::per_changeset_content_blobs_count
                    .add_value(counters.content_blobs_count as i64);
                let root_manifest = manifest_root
                    .clone()
                    .map_ok(Some)
                    .map_err(Error::from)
                    .boxed();

                (entries, root_manifest)
            }
            None => {
                let entry = (manifest_root_id, RepoPath::RootPath);
                (vec![], future::ok(Some(entry)).boxed())
            }
        };

        let buffer_size = tunables::tunables().get_repo_client_concurrent_blob_uploads();
        let s = if buffer_size <= 0 {
            entries
                .into_iter()
                .collect::<FuturesUnordered<_>>()
                .left_stream()
        } else {
            stream::iter(entries)
                .buffer_unordered(buffer_size as usize)
                .right_stream()
        };

        Ok(Self {
            root_manifest,
            sub_entries: s
                .map_err(move |err| {
                    err.context(format!(
                        "While walking dependencies of Root Manifest with id {:?}",
                        manifest_root_id
                    ))
                })
                .boxed(),
        })
    }

    fn walk_helper(
        path_taken: &RepoPath,
        manifest_content: &ManifestContent,
        p1: Option<&ManifestContent>,
        p2: Option<&ManifestContent>,
        manifests: &Manifests,
        filelogs: &Filelogs,
    ) -> Result<(Vec<HgBlobFuture>, WalkHelperCounters)> {
        if path_taken.len() > 4096 {
            bail!(
                "Exceeded max manifest path during walking with path: {:?}",
                path_taken
            );
        }

        let mut entries: Vec<HgBlobFuture> = Vec::new();
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
                            .map_ok(|(id, path)| (Entry::Tree(id), path))
                            .map_err(Error::from)
                            .boxed(),
                    );
                    let (mut walked_entries, sub_counters) = Self::walk_helper(
                        &key.path,
                        manifest_content,
                        get_manifest_parent_content(manifests, key.path.clone(), p1.clone()),
                        get_manifest_parent_content(manifests, key.path.clone(), p2.clone()),
                        manifests,
                        filelogs,
                    )?;
                    entries.append(&mut walked_entries);
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
                            .map_ok(|(id, path)| (Entry::Leaf(id), path))
                            .map_err(Error::from)
                            .boxed(),
                    );
                }
            }
        }

        Ok((entries, counters))
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
) -> Option<ChangesetHandle> {
    match p {
        None => None,
        Some(p) => match map.get(&HgChangesetId::new(p)) {
            None => Some(ChangesetHandle::ready_cs_handle(
                ctx,
                repo.clone(),
                HgChangesetId::new(p),
            )),
            Some(cs) => Some(cs.clone()),
        },
    }
}

pub async fn upload_changeset(
    ctx: CoreContext,
    scribe_category: Option<String>,
    repo: BlobRepo,
    scuba_logger: MononokeScubaSampleBuilder,
    node: HgChangesetId,
    revlog_cs: &RevlogChangeset,
    mut uploaded_changesets: UploadedChangesets,
    filelogs: &Filelogs,
    manifests: &Manifests,
    maybe_backup_repo_source: Option<BlobRepo>,
) -> Result<UploadedChangesets, Error> {
    let NewBlobs {
        root_manifest,
        sub_entries,
    } = NewBlobs::new(revlog_cs.manifestid(), manifests, filelogs)?;

    let cs_metadata = ChangesetMetadata {
        user: String::from_utf8(revlog_cs.user().into())?,
        time: revlog_cs.time().clone(),
        extra: revlog_cs.extra().clone(),
        message: String::from_utf8(revlog_cs.message().into())?,
    };

    // DO NOT try to comute p1 and p2 concurrently!
    // It may result in a combinatoral explosion in mergy repos (see D14100259)
    let p1 = get_parent(ctx.clone(), &repo, &uploaded_changesets, revlog_cs.p1);
    let p2 = get_parent(ctx.clone(), &repo, &uploaded_changesets, revlog_cs.p2);

    let create_bonsai_changeset_hook = Some(create_bonsai_changeset_hook(maybe_backup_repo_source));
    let create_changeset = CreateChangeset {
        expected_nodeid: Some(node.into_nodehash()),
        expected_files: Some(Vec::from(revlog_cs.files())),
        p1,
        p2,
        root_manifest,
        sub_entries,
        // XXX pass content blobs to CreateChangeset here
        cs_metadata,
        create_bonsai_changeset_hook,
        scribe_category,
    };
    let scheduled_uploading = create_changeset.create(ctx, &repo, scuba_logger);

    uploaded_changesets.insert(node, scheduled_uploading);
    Ok(uploaded_changesets)
}
