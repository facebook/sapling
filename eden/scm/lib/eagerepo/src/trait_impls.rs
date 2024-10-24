/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implement traits from other crates.

use cas_client::CasClient;
use cas_client::CasFetchedStats;
use format_util::commit_text_to_root_tree_id;
use format_util::git_sha1_serialize;
use format_util::hg_sha1_serialize;
use format_util::split_hg_file_metadata;
use format_util::strip_file_metadata;
use futures::stream;
use futures::stream::BoxStream;
use futures::StreamExt;
use storemodel::types;
use storemodel::BoxIterator;
use storemodel::FileStore;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::Kind;
use storemodel::ReadRootTreeIds;
use storemodel::SerializationFormat;
use storemodel::TreeStore;
use types::CasDigest;
use types::CasDigestType;
use types::HgId;
use types::Key;
use types::RepoPath;

use crate::EagerRepoStore;

// storemodel traits

impl KeyStore for EagerRepoStore {
    fn get_local_content(
        &self,
        _path: &RepoPath,
        id: HgId,
    ) -> anyhow::Result<Option<minibytes::Bytes>> {
        match self.get_content(id)? {
            Some(data) => {
                let data = match self.format {
                    SerializationFormat::Hg => split_hg_file_metadata(&data).0,
                    SerializationFormat::Git => data,
                };
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    fn insert_data(&self, opts: InsertOpts, _path: &RepoPath, data: &[u8]) -> anyhow::Result<HgId> {
        let sha1_data = match self.format {
            SerializationFormat::Hg => {
                let mut iter = opts.parents.iter();
                let p1 = iter.next().copied().unwrap_or_else(|| *HgId::null_id());
                let p2 = iter.next().copied().unwrap_or_else(|| *HgId::null_id());
                hg_sha1_serialize(data, &p1, &p2)
            }
            SerializationFormat::Git => {
                let type_str = match opts.kind {
                    Kind::File => "blob",
                    Kind::Tree => "tree",
                };
                git_sha1_serialize(data, type_str)
            }
        };

        if let Some(id) = opts.forced_id {
            let id = *id;
            self.add_arbitrary_blob(id, &sha1_data)?;
            Ok(id)
        } else {
            let id = self.add_sha1_blob(&sha1_data, &opts.parents)?;
            Ok(id)
        }
    }

    fn flush(&self) -> anyhow::Result<()> {
        let mut inner = self.inner.write();
        inner.flush()?;
        Ok(())
    }

    fn refresh(&self) -> anyhow::Result<()> {
        let mut inner = self.inner.write();
        inner.flush()?;
        Ok(())
    }

    fn format(&self) -> SerializationFormat {
        self.format
    }

    fn maybe_as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        Box::new(self.clone())
    }
}

impl FileStore for EagerRepoStore {
    fn get_rename_iter(
        &self,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Key)>>> {
        match self.format {
            SerializationFormat::Hg => {
                let store = self.clone();
                let iter = keys.into_iter().filter_map(move |k| {
                    let id = k.hgid;
                    match store.get_content(id) {
                        Err(e) => Some(Err(e.into())),
                        Ok(Some(data)) => match strip_file_metadata(&data, store.format) {
                            Err(e) => Some(Err(e)),
                            Ok((_, Some(copy_from))) => Some(Ok((k, copy_from))),
                            Ok((_, None)) => None,
                        },
                        Ok(None) => Some(Err(anyhow::format_err!("no such file: {:?}", &k))),
                    }
                });
                Ok(Box::new(iter))
            }
            SerializationFormat::Git => Ok(Box::new(std::iter::empty())),
        }
    }

    fn get_hg_parents(&self, _path: &RepoPath, id: HgId) -> anyhow::Result<Vec<HgId>> {
        match self.format {
            SerializationFormat::Hg => {
                let mut parents = Vec::new();
                if let Some(blob) = self.get_sha1_blob(id)? {
                    for start in [HgId::len(), 0] {
                        let end = start + HgId::len();
                        if let Some(slice) = blob.get(start..end) {
                            if let Ok(id) = HgId::from_slice(slice) {
                                if !id.is_null() {
                                    parents.push(id);
                                }
                            }
                        }
                    }
                }
                Ok(parents)
            }
            // For Git, just return a dummy empty "parents".
            SerializationFormat::Git => Ok(Vec::new()),
        }
    }

    fn clone_file_store(&self) -> Box<dyn FileStore> {
        Box::new(self.clone())
    }
}

impl TreeStore for EagerRepoStore {
    fn clone_tree_store(&self) -> Box<dyn TreeStore> {
        Box::new(self.clone())
    }
}

#[async_trait::async_trait]
impl ReadRootTreeIds for EagerRepoStore {
    async fn read_root_tree_ids(&self, commits: Vec<HgId>) -> anyhow::Result<Vec<(HgId, HgId)>> {
        let mut res = Vec::new();
        let format = self.format();
        for commit in &commits {
            let content = self.get_content(*commit)?;
            if let Some(data) = content {
                let tree_id = commit_text_to_root_tree_id(&data, format)?;
                res.push((commit.clone(), tree_id));
            }
        }
        Ok(res)
    }
}

#[async_trait::async_trait]
impl CasClient for EagerRepoStore {
    async fn fetch<'a>(
        &'a self,
        digests: &'a [CasDigest],
        log_name: CasDigestType,
    ) -> BoxStream<
        'a,
        anyhow::Result<(
            CasFetchedStats,
            Vec<(CasDigest, anyhow::Result<Option<Vec<u8>>>)>,
        )>,
    > {
        stream::once(async move {
            tracing::debug!(target: "cas", "EagerRepoStore fetching {} {}(s)", digests.len(), log_name);

            Ok((CasFetchedStats::default(), digests
                .iter()
                .map(|digest| {
                    (
                        *digest,
                        self.get_cas_blob(*digest)
                            .map_err(Into::into)
                            .map(|data| data.map(|data| data.into_vec())),
                    )
                })
                .collect()))
        }).boxed()
    }

    /// Prefetch blobs into the CAS cache
    /// Returns a stream of (stats, digests_prefetched, digests_not_found) tuples
    async fn prefetch<'a>(
        &'a self,
        digests: &'a [CasDigest],
        log_name: CasDigestType,
    ) -> BoxStream<'a, anyhow::Result<(CasFetchedStats, Vec<CasDigest>, Vec<CasDigest>)>> {
        stream::once(async move {
            tracing::debug!(target: "cas", "EagerRepoStore prefetching {} {}(s)", digests.len(), log_name);
            Ok((CasFetchedStats::default(), digests.to_owned(), vec![]))
        }).boxed()
    }
}
