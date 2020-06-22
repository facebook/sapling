/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{
    derive_hg_manifest::derive_hg_manifest, repo_commit::compute_changed_files, BlobRepoHg,
};
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiHgMappingEntry};
use cloned::cloned;
use context::CoreContext;
use futures::future::{self as new_future, FutureExt as NewFutureExt, TryFutureExt};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt, StreamExt};
use futures_old::sync::oneshot;
use futures_old::{
    future::{self, loop_fn, Loop},
    stream, Future, IntoFuture, Stream,
};
use futures_stats::futures01::Timed;
use manifest::{Entry, Manifest, ManifestOps};
use maplit::hashmap;
use mercurial_types::{
    blobs::{
        ChangesetMetadata, ContentBlobMeta, HgBlobChangeset, HgBlobEntry, HgChangesetContent,
        UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash,
    },
    HgChangesetId, HgFileNodeId, HgManifestId, HgParents, Type,
};
use mononoke_types::{BonsaiChangeset, ChangesetId, FileChange, MPath};
use scuba_ext::ScubaSampleBuilderExt;
use slog::debug;
use stats::prelude::*;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};
use time_ext::DurationExt;
use topo_sort::sort_topological;
use tracing::{trace_args, EventId, Traced};

define_stats! {
    prefix = "mononoke.blobrepo";
    get_hg_from_bonsai_changeset: timeseries(Rate, Sum),
    generate_hg_from_bonsai_changeset: timeseries(Rate, Sum),
    generate_hg_from_bonsai_total_latency_ms: histogram(100, 0, 10_000, Average; P 50; P 75; P 90; P 95; P 99),
    generate_hg_from_bonsai_single_latency_ms: histogram(100, 0, 10_000, Average; P 50; P 75; P 90; P 95; P 99),
    generate_hg_from_bonsai_generated_commit_num: histogram(1, 0, 20, Average; P 50; P 75; P 90; P 95; P 99),
}

fn store_file_change(
    repo: &BlobRepo,
    ctx: CoreContext,
    p1: Option<HgFileNodeId>,
    p2: Option<HgFileNodeId>,
    path: &MPath,
    change: &FileChange,
    copy_from: Option<(MPath, HgFileNodeId)>,
) -> impl Future<Item = HgBlobEntry, Error = Error> + Send {
    // If we produced a hg change that has copy info, then the Bonsai should have copy info
    // too. However, we could have Bonsai copy info without having copy info in the hg change
    // if we stripped it out to produce a hg changeset for an Octopus merge and the copy info
    // references a step-parent (i.e. neither p1, not p2).
    if copy_from.is_some() {
        assert!(change.copy_from().is_some());
    }

    // we can reuse same HgFileNodeId if we have only one parent with same
    // file content but different type (Regular|Executable)
    match (p1, p2) {
        (Some(parent), None) | (None, Some(parent)) => {
            let store = repo.get_blobstore().boxed();
            cloned!(ctx, change, path);
            parent
                .load(ctx.clone(), &store)
                .from_err()
                .map(move |parent_envelope| {
                    if parent_envelope.content_id() == change.content_id()
                        && change.copy_from().is_none()
                    {
                        Some(HgBlobEntry::new(
                            store,
                            path.basename().clone(),
                            parent.into_nodehash(),
                            Type::File(change.file_type()),
                        ))
                    } else {
                        None
                    }
                })
                .right_future()
        }
        _ => future::ok(None).left_future(),
    }
    .and_then({
        cloned!(path, change, repo);
        move |maybe_entry| match maybe_entry {
            Some(entry) => future::ok(entry).left_future(),
            None => {
                // Mercurial has complicated logic of finding file parents, especially
                // if a file was also copied/moved.
                // See mercurial/localrepo.py:_filecommit(). We have to replicate this
                // logic in Mononoke.
                // TODO(stash): T45618931 replicate all the cases from _filecommit()

                let parents_fut = if let Some((ref copy_from_path, _)) = copy_from {
                    if copy_from_path != &path && p1.is_some() && p2.is_none() {
                        // This case can happen if a file existed in it's parent
                        // but it was copied over:
                        // ```
                        // echo 1 > 1 && echo 2 > 2 && hg ci -A -m first
                        // hg cp 2 1 --force && hg ci -m second
                        // # File '1' has both p1 and copy from.
                        // ```
                        // In that case Mercurial discards p1 i.e. `hg log` will
                        // use copy from revision as a parent. Arguably not the best
                        // decision, but we have to keep it.
                        future::ok((None, None)).left_future()
                    } else {
                        future::ok((p1, p2)).left_future()
                    }
                } else if p1.is_none() {
                    future::ok((p2, None)).left_future()
                } else if p2.is_some() {
                    crate::file_history::check_if_related(
                        ctx.clone(),
                        repo.clone(),
                        p1.unwrap(),
                        p2.unwrap(),
                        path.clone(),
                    )
                    .map(move |res| {
                        use crate::file_history::FilenodesRelatedResult::*;

                        match res {
                            Unrelated => (p1, p2),
                            FirstAncestorOfSecond => (p2, None),
                            SecondAncestorOfFirst => (p1, None),
                        }
                    })
                    .right_future()
                } else {
                    future::ok((p1, p2)).left_future()
                };

                parents_fut
                    .and_then({
                        move |(p1, p2)| {
                            let upload_entry = UploadHgFileEntry {
                                upload_node_id: UploadHgNodeHash::Generate,
                                contents: UploadHgFileContents::ContentUploaded(ContentBlobMeta {
                                    id: change.content_id(),
                                    size: change.size(),
                                    copy_from: copy_from.clone(),
                                }),
                                file_type: change.file_type(),
                                p1,
                                p2,
                                path: path.clone(),
                            };
                            match upload_entry.upload(ctx, repo.get_blobstore().boxed()) {
                                Ok((_, upload_fut)) => {
                                    upload_fut.map(move |(entry, _)| entry).left_future()
                                }
                                Err(err) => return future::err(err).right_future(),
                            }
                        }
                    })
                    .right_future()
            }
        }
    })
}

/// Check if adding a single path to manifest would cause case-conflict
///
/// Implementation traverses manifest and checks if correspoinding path element is present,
/// if path element is not present, it lowercases current path element and checks if it
/// collides with any existing elements inside manifest. if so it also needs to check that
/// child manifest contains this entry, because it might have been removed.
pub fn check_case_conflict_in_manifest(
    repo: BlobRepo,
    ctx: CoreContext,
    parent_mf_id: HgManifestId,
    child_mf_id: HgManifestId,
    path: MPath,
) -> impl Future<Item = Option<MPath>, Error = Error> {
    let child_mf_id = child_mf_id.clone();
    parent_mf_id
        .load(ctx.clone(), &repo.get_blobstore())
        .from_err()
        .and_then(move |mf| {
            loop_fn(
                (None, mf, path.into_iter()),
                move |(cur_path, mf, mut elements): (Option<MPath>, _, _)| {
                    let element = match elements.next() {
                        None => return future::ok(Loop::Break(None)).boxify(),
                        Some(element) => element,
                    };

                    match mf.lookup(&element) {
                        Some(entry) => {
                            let cur_path = MPath::join_opt_element(cur_path.as_ref(), &element);
                            match entry {
                                Entry::Leaf(..) => future::ok(Loop::Break(None)).boxify(),
                                Entry::Tree(manifest_id) => manifest_id
                                    .load(ctx.clone(), repo.blobstore())
                                    .from_err()
                                    .map(move |mf| Loop::Continue((Some(cur_path), mf, elements)))
                                    .boxify(),
                            }
                        }
                        None => {
                            let element_utf8 = String::from_utf8(Vec::from(element.as_ref()));
                            let mut potential_conflicts = vec![];
                            // Find all entries in the manifests that can potentially be a conflict.
                            // Entry can potentially be a conflict if its lowercased version
                            // is the same as lowercased version of the current element

                            for (basename, _) in mf.list() {
                                let path =
                                    MPath::join_element_opt(cur_path.as_ref(), Some(&basename));
                                match (&element_utf8, std::str::from_utf8(basename.as_ref())) {
                                    (Ok(ref element), Ok(ref basename)) => {
                                        if basename.to_lowercase() == element.to_lowercase() {
                                            potential_conflicts.extend(path);
                                        }
                                    }
                                    _ => (),
                                }
                            }

                            // For each potential conflict we need to check if it's present in
                            // child manifest. If it is, then we've got a conflict, otherwise
                            // this has been deleted and it's no longer a conflict.
                            child_mf_id
                                .find_entries(
                                    ctx.clone(),
                                    repo.get_blobstore(),
                                    potential_conflicts,
                                )
                                .collect()
                                .map(|entries| {
                                    // NOTE: We flatten here because we cannot have a conflict
                                    // at the root.
                                    Loop::Break(entries.into_iter().next().and_then(|x| x.0))
                                })
                                .boxify()
                        }
                    }
                },
            )
        })
}

pub fn get_manifest_from_bonsai(
    repo: &BlobRepo,
    ctx: CoreContext,
    bcs: BonsaiChangeset,
    parent_manifests: Vec<HgManifestId>,
) -> BoxFuture<HgManifestId, Error> {
    let event_id = EventId::new();

    // NOTE: We ignore further parents beyond p1 and p2 for the purposed of tracking copy info
    // or filenode parents. This is because hg supports just 2 parents at most, so we track
    // copy info & filenode parents relative to the first 2 parents, then ignore other parents.

    let (manifest_p1, manifest_p2) = {
        let mut manifests = parent_manifests.iter();
        (manifests.next().copied(), manifests.next().copied())
    };

    let (p1, p2) = {
        let mut parents = bcs.parents();
        let p1 = parents.next();
        let p2 = parents.next();
        (p1, p2)
    };

    // paths *modified* by changeset or *copied from parents*
    let mut p1_paths = Vec::new();
    let mut p2_paths = Vec::new();
    for (path, file_change) in bcs.file_changes() {
        if let Some(file_change) = file_change {
            if let Some((copy_path, bcsid)) = file_change.copy_from() {
                if Some(bcsid) == p1.as_ref() {
                    p1_paths.push(copy_path.clone());
                }
                if Some(bcsid) == p2.as_ref() {
                    p2_paths.push(copy_path.clone());
                }
            };
            p1_paths.push(path.clone());
            p2_paths.push(path.clone());
        }
    }

    let resolve_paths = {
        cloned!(ctx);
        let blobstore = repo.get_blobstore();
        move |maybe_manifest_id: Option<HgManifestId>, paths| match maybe_manifest_id {
            None => future::ok(HashMap::new()).right_future(),
            Some(manifest_id) => manifest_id
                .find_entries(ctx.clone(), blobstore.clone(), paths)
                .filter_map(|(path, entry)| Some((path?, entry.into_leaf()?.1)))
                .collect_to::<HashMap<MPath, HgFileNodeId>>()
                .left_future(),
        }
    };

    // TODO:
    // `derive_manifest` already provides parents for newly created files, so we
    // can remove **all** lookups to files from here, and only leave lookups for
    // files that were copied (i.e bonsai changes that contain `copy_path`)
    let store_file_changes = (
        resolve_paths(manifest_p1, p1_paths),
        resolve_paths(manifest_p2, p2_paths),
    )
        .into_future()
        .traced_with_id(
            &ctx.trace(),
            "generate_hg_manifest::traverse_parents",
            trace_args! {},
            event_id,
        )
        .and_then({
            cloned!(ctx, repo);
            move |(p1s, p2s)| {
                let file_changes: Vec<_> = bcs
                    .file_changes()
                    .map(|(path, file_change)| (path.clone(), file_change.cloned()))
                    .collect();
                stream::iter_ok(file_changes)
                    .map({
                        cloned!(ctx);
                        move |(path, file_change)| match file_change {
                            None => future::ok((path, None)).left_future(),
                            Some(file_change) => {
                                let copy_from =
                                    file_change.copy_from().and_then(|(copy_path, bcsid)| {
                                        if Some(bcsid) == p1.as_ref() {
                                            p1s.get(copy_path).map(|id| (copy_path.clone(), *id))
                                        } else if Some(bcsid) == p2.as_ref() {
                                            p2s.get(copy_path).map(|id| (copy_path.clone(), *id))
                                        } else {
                                            None
                                        }
                                    });
                                store_file_change(
                                    &repo,
                                    ctx.clone(),
                                    p1s.get(&path).cloned(),
                                    p2s.get(&path).cloned(),
                                    &path,
                                    &file_change,
                                    copy_from,
                                )
                                .map(move |entry| (path, Some(entry)))
                                .right_future()
                            }
                        }
                    })
                    .buffer_unordered(100)
                    .collect()
                    .traced_with_id(
                        &ctx.trace(),
                        "generate_hg_manifest::store_file_changes",
                        trace_args! {},
                        event_id,
                    )
            }
        });

    let create_manifest = {
        cloned!(ctx, repo);
        move |changes| {
            derive_hg_manifest(
                ctx.clone(),
                repo.get_blobstore().boxed(),
                parent_manifests,
                changes,
            )
            .traced_with_id(
                &ctx.trace(),
                "generate_hg_manifest::create_manifest",
                trace_args! {},
                event_id,
            )
        }
    };

    store_file_changes
        .and_then(create_manifest)
        .traced_with_id(
            &ctx.trace(),
            "generate_hg_manifest",
            trace_args! {},
            event_id,
        )
        .boxify()
}

fn generate_lease_key(repo: &BlobRepo, bcs_id: &ChangesetId) -> String {
    let repoid = repo.get_repoid();
    format!("repoid.{}.hg-changeset.{}", repoid.id(), bcs_id)
}

fn take_hg_generation_lease(
    repo: BlobRepo,
    ctx: CoreContext,
    bcs_id: ChangesetId,
) -> impl Future<Item = Option<HgChangesetId>, Error = Error> + Send {
    let key = generate_lease_key(&repo, &bcs_id);
    let repoid = repo.get_repoid();

    let derived_data_lease = repo.get_derived_data_lease_ops();
    let bonsai_hg_mapping = repo.get_bonsai_hg_mapping().clone();

    let backoff_ms = 200;
    loop_fn(backoff_ms, move |mut backoff_ms| {
        cloned!(ctx, key);
        derived_data_lease
            .try_add_put_lease(&key)
            .or_else(|_| Ok(false))
            .and_then({
                cloned!(bcs_id, bonsai_hg_mapping, repo);
                move |leased| {
                    let maybe_hg_cs =
                        bonsai_hg_mapping.get_hg_from_bonsai(ctx.clone(), repoid, bcs_id);
                    if leased {
                        maybe_hg_cs
                            .and_then(move |maybe_hg_cs| match maybe_hg_cs {
                                Some(hg_cs) => release_hg_generation_lease(&repo, bcs_id)
                                    .then(move |_| Ok(Loop::Break(Some(hg_cs))))
                                    .left_future(),
                                None => future::ok(Loop::Break(None)).right_future(),
                            })
                            .left_future()
                    } else {
                        maybe_hg_cs
                            .and_then(move |maybe_hg_cs_id| match maybe_hg_cs_id {
                                Some(hg_cs_id) => {
                                    future::ok(Loop::Break(Some(hg_cs_id))).left_future()
                                }
                                None => {
                                    let sleep = rand::random::<u64>() % backoff_ms;
                                    tokio::time::delay_for(Duration::from_millis(sleep))
                                        .then(|_| new_future::ready(Ok(())))
                                        .compat()
                                        .then(move |_: Result<(), Error>| {
                                            backoff_ms *= 2;
                                            if backoff_ms >= 1000 {
                                                backoff_ms = 1000;
                                            }
                                            Ok(Loop::Continue(backoff_ms))
                                        })
                                        .right_future()
                                }
                            })
                            .right_future()
                    }
                }
            })
    })
}

fn renew_hg_generation_lease_forever(
    repo: &BlobRepo,
    ctx: CoreContext,
    bcs_id: ChangesetId,
    done: BoxFuture<(), ()>,
) {
    let key = generate_lease_key(repo, &bcs_id);
    repo.get_derived_data_lease_ops()
        .renew_lease_until(ctx, &key, done)
}

fn release_hg_generation_lease(
    repo: &BlobRepo,
    bcs_id: ChangesetId,
) -> impl Future<Item = (), Error = ()> + Send {
    let key = generate_lease_key(repo, &bcs_id);
    repo.get_derived_data_lease_ops().release_lease(&key)
}

fn generate_hg_changeset(
    repo: BlobRepo,
    ctx: CoreContext,
    bcs_id: ChangesetId,
    bcs: BonsaiChangeset,
    parents: Vec<HgBlobChangeset>,
) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
    let parent_manifests = parents.iter().map(|p| p.manifestid()).collect();

    // NOTE: We're special-casing the first 2 parents here, since that's all Mercurial
    // supports. Producing the Manifest (in get_manifest_from_bonsai) will consider all
    // parents, but everything else is only presented with the first 2 parents, because that's
    // all Mercurial knows about for now. This lets us produce a meaningful Hg changeset for a
    // Bonsai changeset with > 2 parents (which might be one we imported from Git).
    let mut parents = parents.into_iter();
    let p1 = parents.next();
    let p2 = parents.next();

    let p1_hash = p1.as_ref().map(|p1| p1.get_changeset_id());
    let p2_hash = p2.as_ref().map(|p2| p2.get_changeset_id());

    let mf_p1 = p1.map(|p| p.manifestid());
    let mf_p2 = p2.map(|p| p.manifestid());

    let hg_parents = HgParents::new(
        p1_hash.map(|h| h.into_nodehash()),
        p2_hash.map(|h| h.into_nodehash()),
    );

    // Keep a record of any parents for now (i.e. > 2 parents). We'll store those in extras.
    let step_parents = parents;

    get_manifest_from_bonsai(&repo, ctx.clone(), bcs.clone(), parent_manifests)
        .and_then({
            cloned!(ctx, repo);
            move |manifest_id| {
                compute_changed_files(ctx, repo, manifest_id.clone(), mf_p1, mf_p2)
                    .map(move |files| (manifest_id, files))
            }
        })
        // create changeset
        .and_then({
            cloned!(ctx, repo, bcs);
            move |(manifest_id, files)| {
                let mut metadata = ChangesetMetadata {
                    user: bcs.author().to_string(),
                    time: *bcs.author_date(),
                    extra: bcs
                        .extra()
                        .map(|(k, v)| (k.as_bytes().to_vec(), v.to_vec()))
                        .collect(),
                    message: bcs.message().to_string(),
                };

                metadata.record_step_parents(
                    step_parents.into_iter().map(|blob| blob.get_changeset_id()),
                );

                let content =
                    HgChangesetContent::new_from_parts(hg_parents, manifest_id, metadata, files);
                let cs = try_boxfuture!(HgBlobChangeset::new(content));
                let cs_id = cs.get_changeset_id();

                cs.save(ctx.clone(), repo.get_blobstore())
                    .and_then({
                        cloned!(ctx, repo);
                        move |_| {
                            repo.get_bonsai_hg_mapping().add(
                                ctx,
                                BonsaiHgMappingEntry {
                                    repo_id: repo.get_repoid(),
                                    hg_cs_id: cs_id,
                                    bcs_id,
                                },
                            )
                        }
                    })
                    .map(move |_| cs_id)
                    .boxify()
            }
        })
        .traced(
            &ctx.trace(),
            "generate_hg_changeset",
            trace_args! {"changeset" => bcs_id.to_hex().to_string()},
        )
        .timed(move |stats, _| {
            STATS::generate_hg_from_bonsai_single_latency_ms
                .add_value(stats.completion_time.as_millis_unchecked() as i64);
            Ok(())
        })
}

// Converts Bonsai changesets to hg changesets. It either fetches hg changeset id from
// bonsai-hg mapping or it generates hg changeset and puts hg changeset id in bonsai-hg mapping.
// Note that it generates parent hg changesets first.
// This function takes care of making sure the same changeset is not generated at the same time
// by taking leases. It also avoids using recursion to prevents stackoverflow
pub fn get_hg_from_bonsai_changeset_with_impl(
    repo: &BlobRepo,
    ctx: CoreContext,
    bcs_id: ChangesetId,
) -> impl Future<Item = (HgChangesetId, usize), Error = Error> + Send {
    // Finds parent bonsai commits which do not have corresponding hg changeset generated
    // Avoids using recursion
    fn find_toposorted_bonsai_cs_with_no_hg_cs_generated(
        ctx: CoreContext,
        repo: BlobRepo,
        bcs_id: ChangesetId,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    ) -> impl Future<Item = Vec<BonsaiChangeset>, Error = Error> {
        let mut queue = VecDeque::new();
        let mut visited: HashSet<ChangesetId> = HashSet::new();
        visited.insert(bcs_id);
        queue.push_back(bcs_id);

        let repoid = repo.get_repoid();
        loop_fn(
            (queue, vec![], visited),
            move |(mut queue, mut commits_to_generate, mut visited)| {
                cloned!(ctx, repo);
                match queue.pop_front() {
                    Some(bcs_id) => bonsai_hg_mapping
                        .get_hg_from_bonsai(ctx.clone(), repoid, bcs_id)
                        .and_then(move |maybe_hg| match maybe_hg {
                            Some(_hg_cs_id) => {
                                future::ok(Loop::Continue((queue, commits_to_generate, visited)))
                                    .left_future()
                            }
                            None => bcs_id
                                .load(ctx.clone(), repo.blobstore())
                                .from_err()
                                .map(move |bcs| {
                                    commits_to_generate.push(bcs.clone());
                                    queue.extend(bcs.parents().filter(|p| visited.insert(*p)));
                                    Loop::Continue((queue, commits_to_generate, visited))
                                })
                                .right_future(),
                        })
                        .left_future(),
                    None => future::ok(Loop::Break(commits_to_generate)).right_future(),
                }
            },
        )
        .map(|changesets| {
            let mut graph = hashmap! {};
            let mut id_to_bcs = hashmap! {};
            for cs in changesets {
                graph.insert(cs.get_changeset_id(), cs.parents().collect());
                id_to_bcs.insert(cs.get_changeset_id(), cs);
            }
            sort_topological(&graph)
                .expect("commit graph has cycles!")
                .into_iter()
                .map(|cs_id| id_to_bcs.remove(&cs_id))
                .filter_map(|x| x)
                .collect()
        })
    }

    // Panics if changeset not found
    fn fetch_hg_changeset_from_mapping(
        ctx: CoreContext,
        repo: BlobRepo,
        bcs_id: ChangesetId,
    ) -> impl Future<Item = HgBlobChangeset, Error = Error> {
        let bonsai_hg_mapping = repo.get_bonsai_hg_mapping().clone();
        let repoid = repo.get_repoid();
        bonsai_hg_mapping
            .get_hg_from_bonsai(ctx.clone(), repoid, bcs_id)
            .and_then(move |maybe_hg| match maybe_hg {
                Some(hg_cs_id) => hg_cs_id.load(ctx, repo.blobstore()).from_err(),
                None => panic!("hg changeset must be generated already"),
            })
    }

    // Panics if parent hg changesets are not generated
    // Returns whether a commit was generated or not
    fn generate_single_hg_changeset(
        ctx: CoreContext,
        repo: BlobRepo,
        bcs: BonsaiChangeset,
    ) -> impl Future<Item = (HgChangesetId, bool), Error = Error> {
        let bcs_id = bcs.get_changeset_id();

        take_hg_generation_lease(repo.clone(), ctx.clone(), bcs_id.clone())
            .traced(
                &ctx.trace(),
                "create_hg_from_bonsai::wait_for_lease",
                trace_args! {},
            )
            .and_then({
                cloned!(ctx, repo);
                move |maybe_hg_cs_id| {
                    match maybe_hg_cs_id {
                        Some(hg_cs_id) => future::ok((hg_cs_id, false)).left_future(),
                        None => {
                            // We have the lease
                            STATS::generate_hg_from_bonsai_changeset.add_value(1);

                            let mut hg_parents = vec![];
                            for p in bcs.parents() {
                                hg_parents.push(fetch_hg_changeset_from_mapping(
                                    ctx.clone(),
                                    repo.clone(),
                                    p,
                                ));
                            }

                            future::join_all(hg_parents)
                                .and_then({
                                    cloned!(repo);
                                    move |hg_parents| {
                                        let (sender, receiver) = oneshot::channel();

                                        renew_hg_generation_lease_forever(
                                            &repo,
                                            ctx.clone(),
                                            bcs_id,
                                            receiver.map_err(|_| ()).boxify(),
                                        );

                                        generate_hg_changeset(
                                            repo.clone(),
                                            ctx.clone(),
                                            bcs_id,
                                            bcs,
                                            hg_parents,
                                        )
                                        .then(move |res| {
                                            let _ = sender.send(());
                                            res
                                        })
                                    }
                                })
                                .map(|hg_cs_id| (hg_cs_id, true))
                                .right_future()
                        }
                    }
                }
            })
            .timed(move |stats, _| {
                ctx.scuba()
                    .clone()
                    .add_future_stats(&stats)
                    .log_with_msg("Generating hg changeset", Some(format!("{}", bcs_id)));
                Ok(())
            })
    }

    let repoid = repo.get_repoid();
    let bonsai_hg_mapping = repo.get_bonsai_hg_mapping().clone();
    find_toposorted_bonsai_cs_with_no_hg_cs_generated(
        ctx.clone(),
        repo.clone(),
        bcs_id.clone(),
        bonsai_hg_mapping.clone(),
    )
    .and_then({
        cloned!(ctx, repo);
        move |commits_to_generate: Vec<BonsaiChangeset>| {
            let start = (0, commits_to_generate.into_iter());

            loop_fn(
                start,
                move |(mut generated_count, mut commits_to_generate)| match commits_to_generate
                    .next()
                {
                    Some(bcs) => {
                        let bcs_id = bcs.get_changeset_id();

                        generate_single_hg_changeset(ctx.clone(), repo.clone(), bcs)
                            .map({
                                cloned!(ctx);
                                move |(hg_cs_id, generated)| {
                                    if generated {
                                        debug!(
                                            ctx.logger(),
                                            "generated hg changeset for {}: {} ({} left to visit)",
                                            bcs_id,
                                            hg_cs_id,
                                            commits_to_generate.len(),
                                        );
                                        generated_count += 1;
                                    }
                                    Loop::Continue((generated_count, commits_to_generate))
                                }
                            })
                            .left_future()
                    }
                    None => {
                        return bonsai_hg_mapping
                            .get_hg_from_bonsai(ctx.clone(), repoid, bcs_id)
                            .map({
                                cloned!(ctx);
                                move |maybe_hg_cs_id| match maybe_hg_cs_id {
                                    Some(hg_cs_id) => {
                                        if generated_count > 0 {
                                            debug!(
                                                ctx.logger(),
                                                "generation complete for {}", bcs_id,
                                            );
                                        }
                                        Loop::Break((hg_cs_id, generated_count))
                                    }
                                    None => panic!("hg changeset must be generated already"),
                                }
                            })
                            .right_future();
                    }
                },
            )
        }
    })
}

pub(crate) fn get_hg_from_bonsai_changeset(
    repo: &BlobRepo,
    ctx: CoreContext,
    bcs_id: ChangesetId,
) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
    STATS::get_hg_from_bonsai_changeset.add_value(1);
    get_hg_from_bonsai_changeset_with_impl(repo, ctx, bcs_id)
        .map(|(hg_cs_id, generated_commit_num)| {
            STATS::generate_hg_from_bonsai_generated_commit_num
                .add_value(generated_commit_num as i64);
            hg_cs_id
        })
        .timed(move |stats, _| {
            STATS::generate_hg_from_bonsai_total_latency_ms
                .add_value(stats.completion_time.as_millis_unchecked() as i64);
            Ok(())
        })
}
