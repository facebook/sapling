/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bytes::Bytes;
use changesets_creation::save_changesets;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::stream;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use sorted_vector_map::sorted_vector_map;

/// Create (and store) a commit that updates the manifest file on top of `parent`.
/// No bookmark is moved — the caller does that in its atomic transaction.
pub async fn create_manifest_commit(
    ctx: &CoreContext,
    repo: &(
         impl RepoBlobstoreRef
         + FilestoreConfigRef
         + CommitGraphRef
         + CommitGraphWriterRef
         + RepoIdentityRef
     ),
    parent: ChangesetId,
    manifest_path: &NonRootMPath,
    manifest_content: Bytes,
    service_identity: &str,
) -> Result<ChangesetId> {
    let size = manifest_content.len() as u64;

    let metadata = filestore::store(
        repo.repo_blobstore(),
        *repo.filestore_config(),
        ctx,
        &StoreRequest::new(size),
        stream::once(async { Ok(manifest_content) }),
    )
    .await?;

    let file_changes = sorted_vector_map! {
        manifest_path.clone() => FileChange::tracked(
            metadata.content_id,
            FileType::Regular,
            metadata.total_size,
            None,
            GitLfs::FullContent,
        ),
    };

    let bcs_mut = BonsaiChangesetMut {
        parents: vec![parent],
        author: service_identity.to_string(),
        author_date: DateTime::now(),
        committer: None,
        committer_date: None,
        message: format!("Update static manifest at {manifest_path}"),
        hg_extra: Default::default(),
        git_extra_headers: None,
        file_changes,
        is_snapshot: false,
        git_tree_hash: None,
        git_annotated_tag: None,
        subtree_changes: Default::default(),
    };

    let bcs = bcs_mut.freeze()?;
    let cs_id = bcs.get_changeset_id();
    save_changesets(ctx, repo, vec![bcs]).await?;

    Ok(cs_id)
}
