/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use blobstore::{Blobstore, BlobstoreBytes, Loadable};
use cacheblob::MemWritesBlobstore;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    stream::{self, FuturesUnordered, StreamExt, TryStreamExt},
};
use manifest::ManifestOps;
use mononoke_types::{blob::BlobstoreValue, ChangesetId, FsnodeId, MononokeId};
use slog::{debug, info};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[async_trait]
pub trait Cleaner {
    async fn clean(&mut self, cs_ids: Vec<ChangesetId>) -> Result<(), Error>;
}

pub struct FsnodeCleaner {
    alive: HashSet<ChangesetId>,
    children_count: HashMap<ChangesetId, u64>,
    clean_period: u64,
    commits_since_last_clean: u64,
    ctx: CoreContext,
    memblobstore: Arc<MemWritesBlobstore<Arc<dyn Blobstore>>>,
    repo: BlobRepo,
}

impl FsnodeCleaner {
    pub fn new(
        ctx: CoreContext,
        repo: BlobRepo,
        children_count: HashMap<ChangesetId, u64>,
        clean_period: u64,
    ) -> (Self, BlobRepo) {
        let mut memblobstore = None;
        let repo = repo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            let blobstore = Arc::new(MemWritesBlobstore::new(blobstore));
            memblobstore = Some(blobstore.clone());
            blobstore
        });
        let memblobstore = memblobstore.unwrap();

        let s = Self {
            alive: HashSet::new(),
            children_count,
            clean_period,
            commits_since_last_clean: 0,
            ctx,
            repo: repo.clone(),
            memblobstore,
        };

        (s, repo)
    }

    async fn clean_cache(
        &mut self,
        entries_to_preserve: Vec<(String, BlobstoreBytes)>,
    ) -> Result<(), Error> {
        let ctx = &self.ctx;
        let repo = &self.repo;
        {
            let mut cache = self.memblobstore.get_cache().lock().unwrap();
            info!(ctx.logger(), "cache entries: {}", cache.len());
            let mut to_delete = vec![];
            {
                for key in cache.keys() {
                    // That seems to be the best way of detecting if it's fsnode key or not...
                    if key.contains(&FsnodeId::blobstore_key_prefix()) {
                        to_delete.push(key.clone());
                    }
                }
            }

            for key in to_delete {
                cache.remove(&key);
            }
            info!(ctx.logger(), "cache entries after cleanup: {}", cache.len());
        }
        info!(
            ctx.logger(),
            "finished cleanup, preserving {}",
            entries_to_preserve.len()
        );
        stream::iter(entries_to_preserve)
            .map(|(key, value)| {
                debug!(ctx.logger(), "preserving: {}", key);
                // Note - it's important to use repo.get_blobstore() and not
                // use mem_writes blobstore. This is repo.get_blobstore()
                // add a few wrapper blobstores (e.g. the one that adds repo prefix)
                repo.get_blobstore().put(ctx.clone(), key, value).compat()
            })
            .map(Result::<_, Error>::Ok)
            .try_for_each_concurrent(100, |f| async move { f.await })
            .await
    }
}

// Fsnode cleaner for dry-run backfill mode. It's job is to delete all fsnodes entries
// except for those that can be used to derive children commits.
//
// A commit is considered "alive" if there's still at least single child that hasn't
// been derived yet. We must keep all fsnode entries that can be referenced by any alive
// commit. However we are free to delete any other entries.
// Fsnodes cleaner works in the following way:
// 1) It gets a chunk of commits that were just derived and it figures out which commits are still alive
// 2) Periodically (i.e. after every `clean_period` commits) it removes fsnode entries that are no
//    longer reachable by alive commits.
#[async_trait]
impl Cleaner for FsnodeCleaner {
    async fn clean(&mut self, cs_ids: Vec<ChangesetId>) -> Result<(), Error> {
        for cs_id in cs_ids {
            self.commits_since_last_clean += 1;
            let parents = self
                .repo
                .get_changeset_parents_by_bonsai(self.ctx.clone(), cs_id)
                .compat()
                .await?;
            self.alive.insert(cs_id);
            for p in parents {
                let value = if let Some(value) = self.children_count.get_mut(&p) {
                    value
                } else {
                    continue;
                };

                *value -= 1;
                if *value == 0 {
                    self.alive.remove(&p);
                }
            }
        }

        if self.commits_since_last_clean >= self.clean_period {
            self.commits_since_last_clean = 0;
            let entries_to_preserve =
                find_entries_to_preserve(&self.ctx, &self.repo, &self.alive).await?;

            self.clean_cache(entries_to_preserve).await?;
        }

        Ok(())
    }
}

// Finds entries that are still reachable from cs_to_preserve and returns
// corresponding blobs that needs to be saved
async fn find_entries_to_preserve(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_to_preserve: &HashSet<ChangesetId>,
) -> Result<Vec<(String, BlobstoreBytes)>, Error> {
    cs_to_preserve
        .iter()
        .map(|cs_id| async move {
            let root_fsnode = RootFsnodeId::derive(ctx.clone(), repo.clone(), *cs_id)
                .compat()
                .await?;
            Result::<_, Error>::Ok(
                root_fsnode
                    .fsnode_id()
                    .list_tree_entries(ctx.clone(), repo.get_blobstore())
                    .compat()
                    .map_ok(move |(_, mf_id)| async move {
                        let mf = mf_id
                            .load(ctx.clone(), &repo.get_blobstore())
                            .compat()
                            .await?;
                        Ok((mf_id.blobstore_key(), mf.into_blob().into()))
                    })
                    .try_buffer_unordered(100),
            )
        })
        .collect::<FuturesUnordered<_>>()
        .try_flatten()
        .try_collect::<Vec<_>>()
        .await
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo_factory::TestRepoBuilder;
    use fbinit::FacebookInit;
    use futures_old::Stream as OldStream;
    use maplit::hashmap;
    use tests_utils::CreateCommitContext;

    async fn try_list_all_fsnodes(
        ctx: &CoreContext,
        repo: &BlobRepo,
        cs_id: ChangesetId,
    ) -> Result<(), Error> {
        let fsnode = RootFsnodeId::derive(ctx.clone(), repo.clone(), cs_id)
            .compat()
            .await?;
        fsnode
            .fsnode_id()
            .list_all_entries(ctx.clone(), repo.get_blobstore())
            .collect()
            .compat()
            .await?;
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_fsnode_cleaner(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = TestRepoBuilder::new().build()?;

        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file", "content")
            .commit()
            .await?;

        let child = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("file", "content2")
            .commit()
            .await?;

        let clean_period = 1;
        let (cleaner, newrepo) = FsnodeCleaner::new(
            ctx.clone(),
            repo.clone(),
            hashmap! { root => 1 },
            clean_period,
        );
        let mut cleaner = cleaner;
        let repo = newrepo;
        cleaner.clean(vec![root, child]).await?;
        assert!(try_list_all_fsnodes(&ctx, &repo, root).await.is_err());
        assert!(try_list_all_fsnodes(&ctx, &repo, child).await.is_ok());
        Ok(())
    }
}
