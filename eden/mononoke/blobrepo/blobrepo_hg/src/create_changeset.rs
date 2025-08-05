/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use ::manifest::Entry;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::format_err;
use blobstore::Blobstore;
use bonsai_hg_mapping::BonsaiHgMappingArc;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use cloned::cloned;
use commit_graph::CommitGraphWriterArc;
use context::CoreContext;
use futures::channel::oneshot;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream::BoxStream;
use futures_ext::FbTryFutureExt;
use futures_stats::TimedTryFutureExt;
use manifest::ManifestParentReplacement;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::RepoPath;
use mercurial_types::blobs::ChangesetMetadata;
use mercurial_types::subtree::HgSubtreeChanges;
use mononoke_macros::mononoke;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::subtree_change::SubtreeChange;
use repo_blobstore::RepoBlobstoreArc;
use scuba_ext::MononokeScubaSampleBuilder;
use sorted_vector_map::SortedVectorMap;
use stats::prelude::*;
use uuid::Uuid;

use crate::ErrorKind;
use crate::bonsai_generation::create_bonsai_changeset_object;
use crate::bonsai_generation::save_bonsai_changeset_object;
use crate::repo_commit::*;

define_stats! {
    prefix = "mononoke.blobrepo";
    create_changeset: timeseries(Rate, Sum),
    create_changeset_compute_cf: timeseries("create_changeset.compute_changed_files"; Rate, Sum),
    create_changeset_expected_cf: timeseries("create_changeset.expected_changed_files"; Rate, Sum),
    create_changeset_cf_count: timeseries("create_changeset.changed_files_count"; Average, Sum),
}

pub struct CreateChangeset {
    /// This should always be provided, keeping it an Option for tests
    pub expected_nodeid: Option<HgNodeHash>,
    pub expected_files: Option<Vec<NonRootMPath>>,
    pub p1: Option<ChangesetHandle>,
    pub p2: Option<ChangesetHandle>,
    pub subtree_changes: Option<(HgSubtreeChanges, HashMap<HgChangesetId, ChangesetHandle>)>,
    // root_manifest can be None f.e. when commit removes all the content of the repo
    pub root_manifest: BoxFuture<'static, Result<Option<(HgManifestId, RepoPath)>>>,
    pub sub_entries: BoxStream<'static, Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath)>>,
    pub cs_metadata: ChangesetMetadata,
    /// If set to true, don't update Changesets or BonsaiHgMapping, which should be done
    /// manually after this call. Effectively, the commit will be in the blobstore, but
    /// unreachable.
    pub upload_to_blobstore_only: bool,
}

impl CreateChangeset {
    pub fn create(
        self,
        ctx: CoreContext,
        repo: &(impl RepoBlobstoreArc + CommitGraphWriterArc + BonsaiHgMappingArc + Send + Sync),
        bonsai: Option<BonsaiChangeset>,
        mut scuba_logger: MononokeScubaSampleBuilder,
    ) -> ChangesetHandle {
        STATS::create_changeset.add_value(1);
        // This is used for logging, so that we can tie up all our pieces without knowing about
        // the final commit hash
        let uuid = Uuid::new_v4();
        scuba_logger.add("changeset_uuid", format!("{}", uuid));

        let entry_processor =
            UploadEntries::new(repo.repo_blobstore().clone(), scuba_logger.clone());
        let (signal_parent_ready, can_be_parent) = oneshot::channel();
        let signal_parent_ready = Arc::new(Mutex::new(Some(signal_parent_ready)));
        let expected_nodeid = self.expected_nodeid;

        let upload_entries = {
            cloned!(ctx, entry_processor);
            let root_manifest = self.root_manifest;
            let sub_entries = self.sub_entries;
            async move {
                process_entries(&ctx, &entry_processor, root_manifest, sub_entries)
                    .await
                    .context("While processing entries")
            }
        };

        let parents_complete = extract_parents_complete(&self.p1, &self.p2, &self.subtree_changes)
            .try_timed()
            .map({
                let mut scuba_logger = scuba_logger.clone();
                move |result| match result {
                    Err(err) => Err(err.context("While waiting for parents to complete")),
                    Ok((stats, result)) => {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Parents completed", None);
                        Ok(result)
                    }
                }
            });
        let parents_data = handle_parents(scuba_logger.clone(), self.p1, self.p2)
            .map_err(|err| err.context("While waiting for parents to upload data"));

        let changeset = {
            cloned!(ctx, signal_parent_ready, mut scuba_logger);
            let expected_files = self.expected_files;
            let subtree_changes = self.subtree_changes;
            let cs_metadata = self.cs_metadata;
            let blobstore = repo.repo_blobstore_arc();

            async move {
                let (root_mf_id, (parents, parent_manifest_hashes, bonsai_parents)) =
                    future::try_join(upload_entries, parents_data).await?;
                let files = if let Some(expected_files) = expected_files {
                    STATS::create_changeset_expected_cf.add_value(1);
                    // We are trusting the callee to provide a list of changed files, used
                    // by the import job
                    expected_files
                } else if subtree_changes
                    .as_ref()
                    .is_some_and(|(changes, _)| !changes.copies.is_empty())
                {
                    // Presence of subtree copies means the file list is expected to be empty.
                    Vec::new()
                } else {
                    STATS::create_changeset_compute_cf.add_value(1);
                    compute_changed_files(
                        ctx.clone(),
                        blobstore.clone(),
                        root_mf_id,
                        parent_manifest_hashes.first().cloned(),
                        parent_manifest_hashes.get(1).cloned(),
                    )
                    .await?
                };

                let (subtree_replacements, subtree_changes) =
                    resolve_subtree_changes(&ctx, blobstore.clone(), subtree_changes.as_ref())
                        .await?;

                STATS::create_changeset_cf_count.add_value(files.len() as i64);
                let hg_cs = make_new_changeset(parents, root_mf_id, cs_metadata, files)?;

                let (bonsai_cs, bcs_fut) = match bonsai {
                    Some(bonsai_cs) => (bonsai_cs, async move { Ok(()) }.boxed()),
                    None => {
                        let bonsai_cs = create_bonsai_changeset_object(
                            &ctx,
                            hg_cs.clone(),
                            parent_manifest_hashes.clone(),
                            bonsai_parents,
                            subtree_replacements,
                            subtree_changes,
                            &blobstore,
                        )
                        .await?;
                        (
                            bonsai_cs.clone(),
                            save_bonsai_changeset_object(&ctx, &blobstore, bonsai_cs).boxed(),
                        )
                    }
                };
                let bonsai_blob = bonsai_cs.clone().into_blob();
                let bcs_id = bonsai_blob.id().clone();
                let cs_id = hg_cs.get_changeset_id().into_nodehash();
                let manifest_id = hg_cs.manifestid();

                if let Some(expected_nodeid) = expected_nodeid {
                    if cs_id != expected_nodeid {
                        return Err(ErrorKind::InconsistentChangesetHash(
                            expected_nodeid,
                            cs_id,
                            hg_cs,
                        )
                        .into());
                    }
                }

                scuba_logger
                    .add("changeset_id", format!("{}", cs_id))
                    .log_with_msg("Changeset uuid to hash mapping", None);
                // NOTE(luk): an attempt was made in D8187210 to split the
                // upload_entries signal into upload_entries and
                // processed_entries and to signal_parent_ready after
                // upload_entries, so that one doesn't need to wait for the
                // entries to be processed. There were no performance gains
                // from that experiment
                //
                // We deliberately eat this error - this is only so that
                // another changeset can start verifying data in the blob
                // store while we verify this one
                let _ = signal_parent_ready
                    .lock()
                    .expect("poisoned lock")
                    .take()
                    .expect("signal_parent_ready cannot be taken yet")
                    .send(Ok((bcs_id, cs_id, manifest_id)));

                futures::try_join!(
                    bcs_fut,
                    hg_cs.save(&ctx, &blobstore),
                    entry_processor
                        .finalize(&ctx, root_mf_id, parent_manifest_hashes)
                        .map_err(|err| err.context("While finalizing processing")),
                )?;

                Ok::<_, Error>((hg_cs, bonsai_cs))
            }
        }
        .try_timed()
        .map({
            cloned!(mut scuba_logger);
            move |result| {
                match result {
                    Ok((stats, result)) => {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Changeset created", None);
                        Ok(result)
                    }
                    Err(err) => {
                        let err =
                            err.context("While creating and verifying Changeset for blobstore");
                        let trigger = signal_parent_ready.lock().expect("poisoned lock").take();
                        if let Some(trigger) = trigger {
                            // Ignore errors if the receiving end has gone away.
                            let e = format_err!("signal_parent_ready failed: {:?}", err);
                            let _ = trigger.send(Err(e));
                        }
                        Err(err)
                    }
                }
            }
        });

        let commit_graph_writer = repo.commit_graph_writer_arc();
        let bonsai_hg_mapping = repo.bonsai_hg_mapping_arc();
        let changeset_complete_fut = async move {
            let ((hg_cs, bonsai_cs), _) = future::try_join(changeset, parents_complete).await?;

            if !self.upload_to_blobstore_only {
                // update changeset mapping
                commit_graph_writer
                    .add(
                        &ctx,
                        bonsai_cs.get_changeset_id(),
                        bonsai_cs.parents().collect(),
                        bonsai_cs.subtree_sources().collect(),
                    )
                    .await
                    .context("While inserting into changeset table")?;

                // update bonsai mapping
                let bcs_id = bonsai_cs.get_changeset_id();
                let bonsai_hg_entry = BonsaiHgMappingEntry {
                    hg_cs_id: hg_cs.get_changeset_id(),
                    bcs_id,
                };
                bonsai_hg_mapping
                    .add(&ctx, bonsai_hg_entry)
                    .await
                    .context("While inserting mapping")?;
            }

            Ok::<_, Error>((bonsai_cs, hg_cs))
        }
        .try_timed()
        .map({
            cloned!(mut scuba_logger);
            move |result| match result {
                Ok((stats, result)) => {
                    scuba_logger
                        .add_future_stats(&stats)
                        .log_with_msg("CreateChangeset Finished", None);
                    Ok(result)
                }
                Err(err) => Err(err.context(format!(
                    "While creating Changeset {:?}, uuid: {}",
                    expected_nodeid, uuid
                ))),
            }
        });

        let can_be_parent = can_be_parent
            .map(|r| match r {
                Ok(res) => res,
                Err(e) => Err(format_err!("can_be_parent: {:?}", e)),
            })
            .boxed()
            .try_shared();

        let completion_future = mononoke::spawn_task(changeset_complete_fut)
            .map(|result| result?)
            .boxed()
            .try_shared();

        ChangesetHandle::new_pending(can_be_parent, completion_future)
    }
}

/// Convert Mercurial subtree changes into manifest replacements and bonsai subtree changes
async fn resolve_subtree_changes(
    ctx: &CoreContext,
    blobstore: Arc<dyn Blobstore>,
    subtree_changes: Option<&(HgSubtreeChanges, HashMap<HgChangesetId, ChangesetHandle>)>,
) -> Result<(
    Vec<ManifestParentReplacement<HgManifestId, (FileType, HgFileNodeId)>>,
    SortedVectorMap<MPath, SubtreeChange>,
)> {
    if let Some((changes, sources)) = subtree_changes {
        let sources = future::try_join_all(sources.iter().map(|(id, handle)| async move {
            let (bcs, _hg_cs_id, _hg_mf_id) = handle.get_changeset_ids().await?;
            anyhow::Ok((id, bcs))
        }))
        .await?
        .into_iter()
        .collect::<HashMap<_, _>>();
        let manifest_replacements = changes.to_manifest_replacements(ctx, &blobstore).await?;
        let mut subtree_changes = Vec::new();
        for copy in changes.copies.iter() {
            let from_cs_id = sources.get(&copy.from_commit).ok_or_else(|| {
                anyhow!("Subtree copy source commit not found: {}", copy.from_commit)
            })?;
            subtree_changes.push((
                copy.to_path.clone(),
                SubtreeChange::copy(copy.from_path.clone(), *from_cs_id),
            ));
        }
        for deep_copy in changes.deep_copies.iter() {
            let from_cs_id = sources.get(&deep_copy.from_commit).ok_or_else(|| {
                anyhow!(
                    "Subtree deep copy source commit not found: {}",
                    deep_copy.from_commit
                )
            })?;
            subtree_changes.push((
                deep_copy.to_path.clone(),
                SubtreeChange::deep_copy(deep_copy.from_path.clone(), *from_cs_id),
            ));
        }
        for merge in changes.merges.iter() {
            let from_cs_id = sources.get(&merge.from_commit).ok_or_else(|| {
                anyhow!(
                    "Subtree merge source commit not found: {}",
                    merge.from_commit
                )
            })?;
            subtree_changes.push((
                merge.to_path.clone(),
                SubtreeChange::merge(merge.from_path.clone(), *from_cs_id),
            ));
        }
        for import in changes.imports.iter() {
            subtree_changes.push((
                import.to_path.clone(),
                SubtreeChange::import(
                    import.from_path.clone(),
                    import.from_commit.clone(),
                    import.url.clone(),
                ),
            ))
        }
        let subtree_changes = SortedVectorMap::from_iter(subtree_changes.into_iter());
        Ok((manifest_replacements, subtree_changes))
    } else {
        Ok((Vec::new(), SortedVectorMap::new()))
    }
}

#[cfg(test)]
mod tests {
    use blobstore::Loadable;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use manifest::ManifestOps;
    use maplit::hashmap;
    use mercurial_derivation::DeriveHgChangeset;
    use mercurial_types::subtree::HgSubtreeChanges;
    use mercurial_types::subtree::HgSubtreeCopy;
    use mercurial_types::subtree::HgSubtreeDeepCopy;
    use mercurial_types::subtree::HgSubtreeImport;
    use mercurial_types::subtree::HgSubtreeMerge;
    use mononoke_macros::mononoke;
    use repo_blobstore::RepoBlobstoreRef;
    use sorted_vector_map::sorted_vector_map;
    use tests_utils::BasicTestRepo;
    use tests_utils::drawdag::extend_from_dag_with_actions;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_resolve_subtree_changes(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;
        let (commits, _dag) = extend_from_dag_with_actions(
            &ctx,
            &repo,
            r#"
                A-B
                # modify: A dir1/dir2/file1 "file1\n"
                # modify: A dir1/dir2/file2 "file2\n"
                # modify: B dir1/dir3/file3 "file3\n"
            "#,
        )
        .await?;
        let a_id = repo.derive_hg_changeset(&ctx, commits["A"]).await?;
        let b_id = repo.derive_hg_changeset(&ctx, commits["B"]).await?;
        let a_handle = ChangesetHandle::ready_cs_handle(ctx.clone(), repo.clone(), a_id);
        let b_handle = ChangesetHandle::ready_cs_handle(ctx.clone(), repo.clone(), b_id);
        // Handles won't resolve until something fetches the completed changeset.
        a_handle.clone().get_completed_changeset().await?;
        b_handle.clone().get_completed_changeset().await?;

        let (replacements, subtree_changes) = resolve_subtree_changes(
            &ctx,
            repo.repo_blobstore_arc(),
            Some(&(
                HgSubtreeChanges {
                    copies: vec![HgSubtreeCopy {
                        from_path: MPath::new("dir1")?,
                        from_commit: a_id,
                        to_path: MPath::new("dir1a")?,
                    }],
                    deep_copies: vec![
                        HgSubtreeDeepCopy {
                            from_path: MPath::new("dir1/dir2")?,
                            from_commit: a_id,
                            to_path: MPath::new("dir1/dir2a")?,
                        },
                        HgSubtreeDeepCopy {
                            from_path: MPath::new("dir1/dir3")?,
                            from_commit: b_id,
                            to_path: MPath::new("dir1/dir3a")?,
                        },
                    ],
                    merges: vec![HgSubtreeMerge {
                        from_path: MPath::new("dir1/dir2")?,
                        from_commit: a_id,
                        to_path: MPath::new("dir1/dir3")?,
                    }],
                    imports: vec![HgSubtreeImport {
                        from_path: MPath::new("otherdir")?,
                        from_commit: "other commit".to_string(),
                        url: "other:repo".to_string(),
                        to_path: MPath::new("dir4")?,
                    }],
                },
                hashmap! {
                    a_id => a_handle,
                    b_id => b_handle,
                },
            )),
        )
        .await?;

        assert_eq!(
            replacements,
            vec![ManifestParentReplacement {
                path: MPath::new("dir1a")?,
                replacements: vec![
                    a_id.load(&ctx, repo.repo_blobstore())
                        .await?
                        .manifestid()
                        .find_entry(ctx.clone(), repo.repo_blobstore_arc(), MPath::new("dir1")?)
                        .await?
                        .unwrap()
                ],
            }]
        );
        assert_eq!(
            subtree_changes,
            sorted_vector_map! {
                MPath::new("dir1/dir2a")? => SubtreeChange::deep_copy(MPath::new("dir1/dir2")?, commits["A"]),
                MPath::new("dir1/dir3")? => SubtreeChange::merge( MPath::new("dir1/dir2")?, commits["A"] ),
                MPath::new("dir1/dir3a")? => SubtreeChange::deep_copy(MPath::new("dir1/dir3")?, commits["B"]),
                MPath::new("dir1a")? =>  SubtreeChange::copy( MPath::new("dir1")?, commits["A"]),
                MPath::new("dir4")? => SubtreeChange::import(MPath::new("otherdir")?, "other commit".to_string(), "other:repo".to_string()),
            }
        );

        Ok(())
    }
}
