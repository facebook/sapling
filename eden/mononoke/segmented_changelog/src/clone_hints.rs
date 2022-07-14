/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use bonsai_hg_mapping::BonsaiHgMapping;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mercurial_types::HgChangesetId;
use mononoke_types::hash;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use slog::debug;
use slog::info;
use sql::queries;
use sql_ext::SqlConnections;
use std::collections::HashMap;
use std::sync::Arc;

use crate::idmap::IdMap;
use crate::types::IdMapVersion;
use crate::update::ServerNameDag;
use crate::DagId;

/// Number of hint entries to store in a single chunk.
///
/// Each chunk will have exactly this many hints in it - we do not save partial chunks.
const HINTS_PER_CHUNK: usize = 5000;

queries! {
    read SelectChunks(repo_id: RepositoryId, version: u64) -> (Vec<u8>) {
        "SELECT blob_name
        FROM segmented_changelog_clone_hints
        WHERE repo_id = {repo_id}
        AND idmap_version = {version}"
    }

    write InsertChunks(values: (repo_id: RepositoryId, version: u64, blob_name: &str)) {
        none,
        "INSERT INTO segmented_changelog_clone_hints
        (repo_id, idmap_version, blob_name)
        VALUES {values}"
    }
}

#[derive(Clone)]
pub struct CloneHints {
    inner: Arc<CloneHintsInner>,
}

struct CloneHintsInner {
    connections: SqlConnections,
    repo_id: RepositoryId,
    blobstore: Arc<dyn Blobstore>,
}

impl CloneHints {
    pub fn new(
        connections: SqlConnections,
        repo_id: RepositoryId,
        blobstore: Arc<dyn Blobstore>,
    ) -> Self {
        let inner = Arc::new(CloneHintsInner {
            connections,
            repo_id,
            blobstore,
        });
        Self { inner }
    }

    pub(crate) async fn fetch(
        &self,
        ctx: &CoreContext,
        idmap_version: IdMapVersion,
    ) -> Result<HashMap<DagId, (ChangesetId, HgChangesetId)>> {
        let hint_chunk_names = SelectChunks::query(
            &self.inner.connections.read_connection,
            &self.inner.repo_id,
            &idmap_version.0,
        )
        .await?;

        let mut hints = HashMap::new();
        stream::iter(hint_chunk_names)
            .map(move |(name,)| async move {
                self.inner
                    .blobstore
                    .get(
                        ctx,
                        std::str::from_utf8(&name).expect("Name should be UTF-8"),
                    )
                    .await
            })
            .buffer_unordered(100)
            .try_fold(&mut hints, |hints, blob| async move {
                if let Some(blob) = blob {
                    let blob = blob.as_raw_bytes();
                    let values: Vec<Hint> = mincode::deserialize(blob)?;
                    for hint in values {
                        hints.insert(
                            DagId(hint.dag_id),
                            (
                                ChangesetId::from_bytes(hint.cs_id)?,
                                HgChangesetId::from_bytes(&hint.hgcs_id)?,
                            ),
                        );
                    }
                }
                Ok(hints)
            })
            .await?;

        Ok(hints)
    }

    pub(crate) async fn add_hints(
        &self,
        ctx: &CoreContext,
        namedag: &ServerNameDag,
        idmap_version: IdMapVersion,
        bonsai_hg_mapping: &dyn BonsaiHgMapping,
    ) -> Result<()> {
        info!(
            ctx.logger(),
            "Adding hints for idmap_version {}", idmap_version.0
        );

        // Similar to `export_pull_data` in the dag crate.
        let id_dag = namedag.dag();
        let all_ids = id_dag.all().context("error computing all() ids")?;
        let flat_segments = id_dag
            .idset_to_flat_segments(all_ids)
            .context("error getting segments for all()")?;
        let ids: Vec<_> = flat_segments.parents_head_and_roots().into_iter().collect();
        let universal_ids_len = ids.len();
        let existing_hints = self.fetch(ctx, idmap_version).await?;
        let new_ids: Vec<_> = ids
            .into_iter()
            .filter(|id| !existing_hints.contains_key(id))
            .collect();

        if new_ids.len() < HINTS_PER_CHUNK {
            info!(
                ctx.logger(),
                "idmap_version {} has a full set of hints ({} unhinted IDs is less than chunk size of {})",
                idmap_version.0,
                new_ids.len(),
                HINTS_PER_CHUNK
            );
            return Ok(());
        }

        debug!(
            ctx.logger(),
            "Found {} universally known IDs, {} in existing hints, {} to find",
            universal_ids_len,
            existing_hints.len(),
            new_ids.len()
        );

        let idmap_entries = namedag
            .map()
            .as_inner()
            .find_many_changeset_ids(ctx, new_ids.clone())
            .await
            .context("error retrieving mappings for dag universal ids")?;

        let csids: Vec<_> = idmap_entries.values().copied().collect();
        let hg_mapping: HashMap<_, _> = bonsai_hg_mapping
            .get(ctx, csids.into())
            .await
            .context("error converting from bonsai to hg")?
            .into_iter()
            .map(|mapping| (mapping.bcs_id, mapping.hg_cs_id))
            .collect();

        let new_hints: Vec<Hint> = new_ids
            .into_iter()
            .filter_map(|dag_id| {
                let cs_id = idmap_entries.get(&dag_id)?;
                let hgcs_id = hg_mapping.get(cs_id)?;
                let dag_id = dag_id.0;
                let cs_id = cs_id.blake2().into_inner();
                let hgcs_id = hgcs_id.as_bytes().try_into().ok()?;
                Some(Hint {
                    dag_id,
                    cs_id,
                    hgcs_id,
                })
            })
            .collect();

        debug!(ctx.logger(), "Uploading {} hint entries", new_hints.len());

        let hint_blob_keys: Vec<_> = stream::iter(new_hints.chunks_exact(HINTS_PER_CHUNK).map(
            |chunk| async move {
                let chunk: Vec<_> = chunk.iter().collect();
                let chunk = mincode::serialize(&chunk)?;
                let chunk_hash = {
                    let mut context = hash::Context::new(b"segmented_clone");
                    context.update(&chunk);
                    context.finish()
                };
                let chunk_key =
                    format!("segmented_clone_v1_idmapv{}.{}", idmap_version, chunk_hash);
                let blob = BlobstoreBytes::from_bytes(chunk);
                self.inner
                    .blobstore
                    .put(ctx, chunk_key.clone(), blob)
                    .await?;
                debug!(ctx.logger(), "Uploaded hint entry {}", &chunk_key);
                Ok::<_, Error>(chunk_key)
            },
        ))
        .buffer_unordered(100)
        .try_collect()
        .await?;

        let hint_blob_keys: Vec<&str> = hint_blob_keys.iter().map(|s| s.as_str()).collect();

        let hint_blob_values: Vec<_> = hint_blob_keys
            .iter()
            .map(|key| (&self.inner.repo_id, &idmap_version.0, key))
            .collect();
        InsertChunks::query(
            &self.inner.connections.write_connection,
            &hint_blob_values[..],
        )
        .await?;

        info!(ctx.logger(), "Hints uploaded",);
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct Hint {
    dag_id: u64,
    cs_id: [u8; 32],
    hgcs_id: [u8; 20],
}
