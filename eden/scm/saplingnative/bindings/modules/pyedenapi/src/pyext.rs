/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::bail;
use anyhow::format_err;
use async_runtime::block_unless_interrupted;
use cpython::*;
use cpython_async::PyFuture;
use cpython_async::TStream;
use cpython_ext::ExtractInner;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use cpython_ext::convert::Serde;
use dag_types::Location;
use edenapi::Response;
use edenapi::SaplingRemoteApi;
use edenapi::SaplingRemoteApiError;
use edenapi::Stats;
use edenapi::UploadLookupPolicy;
use edenapi_ext::calc_contentid;
use edenapi_types::AlterSnapshotRequest;
use edenapi_types::AlterSnapshotResponse;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::BookmarkResult;
use edenapi_types::CloudShareWorkspaceRequest;
use edenapi_types::CloudShareWorkspaceResponse;
use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitGraphSegmentsEntry;
use edenapi_types::CommitHashLookupResponse;
use edenapi_types::CommitHashToLocationResponse;
use edenapi_types::CommitKnownResponse;
use edenapi_types::CommitLocationToHashRequest;
use edenapi_types::CommitLocationToHashResponse;
use edenapi_types::CommitRevlogData;
use edenapi_types::FetchSnapshotRequest;
use edenapi_types::FetchSnapshotResponse;
use edenapi_types::FileResponse;
use edenapi_types::GetReferencesParams;
use edenapi_types::GetSmartlogByVersionParams;
use edenapi_types::GetSmartlogParams;
use edenapi_types::HgChangesetContent;
use edenapi_types::HgFilenodeData;
use edenapi_types::HgMutationEntryContent;
use edenapi_types::HistoricalVersionsParams;
use edenapi_types::HistoricalVersionsResponse;
use edenapi_types::HistoryEntry;
use edenapi_types::IndexableId;
use edenapi_types::LandStackResponse;
use edenapi_types::LookupResult;
use edenapi_types::PathHistoryRequestPaginationCursor;
use edenapi_types::PathHistoryResponse;
use edenapi_types::ReferencesDataResponse;
use edenapi_types::RenameWorkspaceRequest;
use edenapi_types::RenameWorkspaceResponse;
use edenapi_types::RollbackWorkspaceRequest;
use edenapi_types::RollbackWorkspaceResponse;
use edenapi_types::SaplingRemoteApiServerError;
use edenapi_types::SetBookmarkResponse;
use edenapi_types::TreeAttributes;
use edenapi_types::TreeEntry;
use edenapi_types::UpdateArchiveParams;
use edenapi_types::UpdateArchiveResponse;
use edenapi_types::UpdateReferencesParams;
use edenapi_types::UploadHgChangeset;
use edenapi_types::UploadToken;
use edenapi_types::WorkspaceDataResponse;
use edenapi_types::WorkspacesDataResponse;
use edenapi_types::bookmark::Freshness;
use edenapi_types::cloud::SmartlogDataResponse;
use format_util::split_hg_file_metadata;
use futures::prelude::*;
use progress_model::ProgressBar;
use pyrevisionstore::filescmstore;
use revisionstore::HgIdDataStore;
use revisionstore::HgIdMutableDeltaStore;
use revisionstore::StoreKey;
use revisionstore::StoreResult;
use sha1::Digest;
use types::FetchContext;
use types::HgId;

use crate::pytypes::PyStats;
use crate::stats::stats;
use crate::util::as_deltastore;
use crate::util::meta_to_dict;
use crate::util::to_contentid;
use crate::util::to_keys;
use crate::util::to_keys_with_parents;
use crate::util::to_path;
use crate::util::to_trees_upload_items;

/// Extension trait allowing SaplingRemoteAPI methods to be called from Python code.
///
/// One nice benefit of making this a trait instead of directly implementing
/// the methods inside a `py_class!` macro invocation is that tools like
/// `rustfmt` can still parse the code.
pub trait SaplingRemoteApiPyExt: SaplingRemoteApi {
    fn health_py(&self, py: Python) -> PyResult<PyDict> {
        let meta = block_unless_interrupted(self.health())
            .map_pyerr(py)?
            .map_pyerr(py)?;
        meta_to_dict(py, &meta)
    }

    fn files_py(
        &self,
        py: Python,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
    ) -> PyResult<TStream<anyhow::Result<Serde<FileResponse>>>> {
        let keys = to_keys(py, &keys)?;
        let entries = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let fctx = FetchContext::sapling_default();
                    self.files(fctx, keys).await
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?
            .entries;
        Ok(entries.map_ok(Serde).map_err(Into::into).into())
    }

    fn history_py(
        &self,
        py: Python,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
        length: Option<u32>,
    ) -> PyResult<TStream<anyhow::Result<Serde<HistoryEntry>>>> {
        let keys = to_keys(py, &keys)?;
        let entries = py
            .allow_threads(|| block_unless_interrupted(self.history(keys, length)))
            .map_pyerr(py)?
            .map_pyerr(py)?
            .entries;
        Ok(entries.map_ok(Serde).map_err(Into::into).into())
    }

    fn path_history_py(
        &self,
        py: Python,
        commit: Serde<HgId>,
        paths: Vec<PyPathBuf>,
        limit: Option<u32>,
        cursor: Vec<Serde<PathHistoryRequestPaginationCursor>>,
    ) -> PyResult<TStream<anyhow::Result<Serde<PathHistoryResponse>>>> {
        let paths = paths
            .into_iter()
            .map(|path| to_path(py, &path).unwrap())
            .collect();
        let cursor = cursor.into_iter().map(|c| c.0).collect();

        let entries = py
            .allow_threads(|| {
                block_unless_interrupted(self.path_history(commit.0, paths, limit, cursor))
            })
            .map_pyerr(py)?
            .map_pyerr(py)?
            .entries;
        Ok(entries.map_ok(Serde).map_err(Into::into).into())
    }

    fn storetrees_py(
        &self,
        py: Python,
        store: PyObject,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
        attributes: Option<TreeAttributes>,
    ) -> PyResult<stats> {
        let keys = to_keys(py, &keys)?;
        let store = as_deltastore(py, store)?;

        let prog =
            ProgressBar::new_adhoc("Downloading trees over HTTP", keys.len() as u64, "trees");
        let prog = (*prog).clone();

        let stats = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let fctx = FetchContext::sapling_default();
                    let response = self.trees(fctx, keys, attributes).await?;
                    write_trees(response, store, prog).await
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        stats::new(py, stats)
    }

    fn trees_py(
        &self,
        py: Python,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
        attributes: Option<TreeAttributes>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<TreeEntry>>>, PyFuture)> {
        let keys = to_keys(py, &keys)?;
        let (trees, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let fctx = FetchContext::sapling_default();
                    let response = self.trees(fctx, keys, attributes).await?;
                    Ok::<_, SaplingRemoteApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let trees_py = trees.map(|t| match t {
            Ok(Ok(t)) => Ok(Serde(t)),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(e.into()),
        });
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((trees_py.into(), stats_py))
    }

    fn commit_revlog_data_py(
        &self,
        py: Python,
        nodes: Vec<HgId>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitRevlogData>>>, PyFuture)> {
        let (commits, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.commit_revlog_data(nodes).await?;
                    let commits = response.entries;
                    let stats = response.stats;
                    Ok::<_, SaplingRemoteApiError>((commits, stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let commits_py = commits.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((commits_py.into(), stats_py))
    }

    fn bookmarks_py(&self, py: Python, bookmarks: Vec<String>) -> PyResult<PyDict> {
        let response = py
            .allow_threads(|| block_unless_interrupted(self.bookmarks(bookmarks)))
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let bookmarks = PyDict::new(py);
        for entry in response {
            bookmarks.set_item(py, entry.bookmark, entry.hgid.map(|id| id.to_hex()))?;
        }
        Ok(bookmarks)
    }

    fn bookmarks2_py(
        &self,
        py: Python,
        bookmarks: Vec<String>,
        freshness: Option<Freshness>,
    ) -> PyResult<Serde<Vec<BookmarkResult>>> {
        let items = py
            .allow_threads(|| block_unless_interrupted(self.bookmarks2(bookmarks, freshness)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(items))
    }

    fn set_bookmark_py(
        &self,
        py: Python,
        bookmark: String,
        to: Option<HgId>,
        from: Option<HgId>,
        pushvars: HashMap<String, String>,
    ) -> PyResult<Serde<SetBookmarkResponse>> {
        let response = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.set_bookmark(bookmark, to, from, pushvars).await?;
                    Ok::<_, SaplingRemoteApiError>(response)
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        Ok(Serde(response))
    }

    fn land_stack_py(
        &self,
        py: Python,
        bookmark: String,
        head: HgId,
        base: HgId,
        pushvars: HashMap<String, String>,
    ) -> PyResult<Serde<LandStackResponse>> {
        let response = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.land_stack(bookmark, head, base, pushvars).await?;
                    Ok::<_, SaplingRemoteApiError>(response)
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        Ok(Serde(response))
    }

    fn hash_lookup_py(
        &self,
        py: Python,
        hash_prefixes: Vec<String>,
    ) -> PyResult<Serde<Vec<CommitHashLookupResponse>>> {
        let items = py
            .allow_threads(|| block_unless_interrupted(self.hash_prefixes_lookup(hash_prefixes)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(items))
    }

    fn commit_location_to_hash_py(
        &self,
        py: Python,
        requests: Vec<(HgId, u64, u64)>,
    ) -> PyResult<Serde<Vec<CommitLocationToHashResponse>>> {
        let requests = requests
            .into_iter()
            .map(|(hgid, distance, count)| {
                let location = Location::new(hgid, distance);
                CommitLocationToHashRequest { location, count }
            })
            .collect();
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.commit_location_to_hash(requests)))
            .map_pyerr(py)?
            .map_pyerr(py)?;

        Ok(Serde(responses))
    }

    fn commit_hash_to_location_py(
        &self,
        py: Python,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
    ) -> PyResult<Serde<Vec<CommitHashToLocationResponse>>> {
        let responses = py
            .allow_threads(|| {
                block_unless_interrupted(self.commit_hash_to_location(master_heads, hgids))
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        Ok(Serde(responses))
    }

    fn commit_known_py(
        &self,
        py: Python,
        hgids: Vec<HgId>,
    ) -> PyResult<Serde<Vec<CommitKnownResponse>>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.commit_known(hgids)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn commit_graph_py(
        &self,
        py: Python,
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> PyResult<Serde<Vec<CommitGraphEntry>>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.commit_graph(heads, common)))
            .map_pyerr(py)?
            .map_pyerr(py)?;

        Ok(Serde(responses))
    }

    fn commit_graph_segments_py(
        &self,
        py: Python,
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> PyResult<Serde<Vec<CommitGraphSegmentsEntry>>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.commit_graph_segments(heads, common)))
            .map_pyerr(py)?
            .map_pyerr(py)?;

        Ok(Serde(responses))
    }

    fn lookup_file_contents(
        &self,
        py: Python,
        ids: Vec<PyBytes>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        self.lookup_py(
            py,
            ids.into_iter()
                .map(|id| {
                    AnyId::AnyFileContentId(AnyFileContentId::ContentId(to_contentid(py, &id)))
                })
                .collect(),
        )
    }

    fn lookup_commits(
        &self,
        py: Python,
        nodes: Vec<HgId>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        self.lookup_py(py, nodes.into_iter().map(AnyId::HgChangesetId).collect())
    }

    fn lookup_filenodes(
        &self,
        py: Python,
        ids: Vec<HgId>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        self.lookup_py(py, ids.into_iter().map(AnyId::HgFilenodeId).collect())
    }

    fn lookup_trees(
        &self,
        py: Python,
        ids: Vec<HgId>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        self.lookup_py(py, ids.into_iter().map(AnyId::HgTreeId).collect())
    }

    fn lookup_filenodes_and_trees(
        &self,
        py: Python,
        filenodes_ids: Vec<HgId>,
        trees_ids: Vec<HgId>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        self.lookup_py(
            py,
            filenodes_ids
                .into_iter()
                .map(AnyId::HgFilenodeId)
                .chain(trees_ids.into_iter().map(AnyId::HgTreeId))
                .collect(),
        )
    }

    fn lookup_py(&self, py: Python, ids: Vec<AnyId>) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        let responses = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    // Deduplicates input, keeping indexes
                    let indexable_ids: HashMap<IndexableId, Vec<usize>> = ids
                        .iter()
                        .cloned()
                        .map(|id| IndexableId {
                            id,
                            bubble_id: None,
                        })
                        .enumerate()
                        .fold(HashMap::new(), |mut map, (idx, id)| {
                            map.entry(id).or_default().push(idx);
                            map
                        });
                    self.lookup_batch(
                        indexable_ids.keys().map(|id| id.id.clone()).collect(),
                        None,
                        None,
                    )
                    .await
                    .map(|responses| {
                        responses
                            .into_iter()
                            .flat_map(|r| match r.result {
                                LookupResult::Present(token) => indexable_ids
                                    .get(&token.indexable_id())
                                    .cloned()
                                    .unwrap_or_default()
                                    .into_iter()
                                    .map(|idx| (idx, token.clone()))
                                    .collect(),
                                LookupResult::NotPresent(_) => vec![],
                            })
                            .collect()
                    })
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn uploadfiles_py(
        &self,
        py: Python,
        store: filescmstore,
        keys: Vec<(
            PyPathBuf,   /* path */
            Serde<HgId>, /* hgid */
            Serde<HgId>, /* p1 */
            Serde<HgId>, /* p2 */
        )>,
        use_sha1: bool,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        let keys = to_keys_with_parents(py, &keys)?;
        let store = store.extract_inner(py);

        let file_content_id = |data: &[u8]| -> AnyFileContentId {
            if use_sha1 {
                let mut hasher = sha1::Sha1::new();
                hasher.update(data);
                let hash: [u8; 20] = hasher.finalize().into();
                AnyFileContentId::Sha1(hash.into())
            } else {
                AnyFileContentId::ContentId(calc_contentid(data))
            }
        };

        // Preupload LFS blobs
        store
            .upload_lfs(
                &keys
                    .iter()
                    .map(|(key, _)| StoreKey::hgid(key.clone()))
                    .collect::<Vec<_>>(),
            )
            .map_pyerr(py)?;

        let (mut upload_data, filenodes_data): (Vec<_>, Vec<_>) = keys
            .into_iter()
            .map(|(key, parents)| {
                let content = store.get(StoreKey::hgid(key.clone())).map_pyerr(py)?;
                match content {
                    StoreResult::Found(raw_content) => {
                        let raw_content = raw_content.into();
                        let (raw_data, copy_from) = split_hg_file_metadata(&raw_content);
                        let content_id = file_content_id(&raw_data);
                        Ok((
                            (content_id, raw_data),
                            (key.hgid, content_id, parents, copy_from),
                        ))
                    }
                    _ => Err(SaplingRemoteApiError::Other(format_err!(
                        "failed to fetch file content for the key '{}'",
                        key
                    )))
                    .map_pyerr(py),
                }
            })
            // fail the entire operation if content is missing for some key (filenode) because this is unexpected
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();

        // Deduplicate upload data
        let mut uniques = HashSet::new();
        upload_data.retain(|(content_id, _)| uniques.insert(*content_id));

        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let downcast_error = "incorrect upload token, failed to downcast 'token.data.id' to 'AnyId::AnyFileContentId::ContentId' type";
                    // upload file contents first, receiving upload tokens
                    let file_content_tokens = self
                        .process_files_upload(upload_data, None, None, UploadLookupPolicy::PerformLookup)
                        .await?
                        .entries
                        .try_collect::<Vec<_>>()
                        .await?
                        .into_iter()
                        .map(|token| {
                            let content_id = match token.data.id {
                                AnyId::AnyFileContentId(id) => id,
                                _ => bail!(SaplingRemoteApiError::Other(format_err!(downcast_error))),
                            };
                            Ok((content_id, token))
                        })
                        .collect::<Result<HashMap<_, _>, _>>()?;

                    // build the list of HgFilenodeData for upload
                    let filenodes_data = filenodes_data.into_iter().map(|(node_id, content_id, parents, copy_from)| {
                        let file_content_upload_token = file_content_tokens.get(&content_id).ok_or_else(|| SaplingRemoteApiError::Other(format_err!("unexpected error: upload token is missing for ContentId({})", content_id)))?.clone();
                        Ok(HgFilenodeData {
                            node_id,
                            parents,
                            file_content_upload_token,
                            metadata: copy_from.to_vec(),
                        })
                    }).collect::<Result<Vec<_>, SaplingRemoteApiError>>()?;

                    // upload hg filenodes
                    let response = self
                        .upload_filenodes_batch(filenodes_data)
                        .await?;

                    Ok::<_, SaplingRemoteApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(|r| Serde(r.token)).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    /// Upload trees
    fn uploadtrees_py(
        &self,
        py: Python,
        items: Vec<(
            Serde<HgId>, /* hgid */
            Serde<HgId>, /* p1 */
            Serde<HgId>, /* p2 */
            PyBytes,     /* data */
        )>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        let items = to_trees_upload_items(py, &items)?;
        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.upload_trees_batch(items).await?;
                    Ok::<_, SaplingRemoteApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(|r| Serde(r.token)).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    /// Upload changesets
    /// This method sends a single request, batching should be done outside.
    fn uploadchangesets_py(
        &self,
        py: Python,
        changesets: Vec<(
            HgId,               /* hgid (node_id) */
            HgChangesetContent, /* changeset content */
        )>,
        mutations: Vec<Serde<HgMutationEntryContent>>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        let changesets = changesets
            .into_iter()
            .map(|(node_id, content)| UploadHgChangeset {
                node_id,
                changeset_content: content,
            })
            .collect();
        let mutations = mutations.into_iter().map(|m| m.0).collect();
        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.upload_changesets(changesets, mutations).await?;
                    Ok::<_, SaplingRemoteApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(|r| Serde(r.token)).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    fn fetchsnapshot_py(
        &self,
        py: Python,
        data: Serde<FetchSnapshotRequest>,
    ) -> PyResult<Serde<FetchSnapshotResponse>> {
        py.allow_threads(|| {
            block_unless_interrupted(async move {
                let cs_id = data.0.cs_id;
                self.fetch_snapshot(data.0)
                    .await
                    .with_context(|| format_err!("Failed to find snapshot {}", cs_id))
            })
        })
        .map_pyerr(py)?
        .map_pyerr(py)
        .map(Serde)
    }

    fn altersnapshot_py(
        &self,
        py: Python,
        data: Serde<AlterSnapshotRequest>,
    ) -> PyResult<Serde<AlterSnapshotResponse>> {
        py.allow_threads(|| {
            block_unless_interrupted(async move {
                let cs_id = data.0.cs_id;
                self.alter_snapshot(data.0)
                    .await
                    .with_context(|| format_err!("Failed to alter snapshot {}", cs_id))
            })
        })
        .map_pyerr(py)?
        .map_pyerr(py)
        .map(Serde)
    }

    fn downloadfiletomemory_py(&self, py: Python, token: Serde<UploadToken>) -> PyResult<PyBytes> {
        py.allow_threads(|| block_unless_interrupted(self.download_file(token.0)))
            .map_pyerr(py)?
            .map_pyerr(py)
            .map(|data| PyBytes::new(py, &data))
    }

    fn cloud_workspace_py(
        &self,
        workspace: String,
        reponame: String,
        py: Python,
    ) -> PyResult<Serde<WorkspaceDataResponse>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.cloud_workspace(workspace, reponame)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn cloud_workspaces_py(
        &self,
        prefix: String,
        reponame: String,
        py: Python,
    ) -> PyResult<Serde<WorkspacesDataResponse>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.cloud_workspaces(prefix, reponame)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn cloud_references_py(
        &self,
        data: Serde<GetReferencesParams>,
        py: Python,
    ) -> PyResult<Serde<ReferencesDataResponse>> {
        py.allow_threads(|| block_unless_interrupted(self.cloud_references(data.0)))
            .map_pyerr(py)?
            .map_pyerr(py)
            .map(Serde)
    }

    fn cloud_update_references_py(
        &self,
        data: Serde<UpdateReferencesParams>,
        py: Python,
    ) -> PyResult<Serde<ReferencesDataResponse>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.cloud_update_references(data.0)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn cloud_smartlog_py(
        &self,
        data: Serde<GetSmartlogParams>,
        py: Python,
    ) -> PyResult<Serde<SmartlogDataResponse>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.cloud_smartlog(data.0)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn cloud_share_workspace_py(
        &self,
        data: Serde<CloudShareWorkspaceRequest>,
        py: Python,
    ) -> PyResult<Serde<CloudShareWorkspaceResponse>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.cloud_share_workspace(data.0)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn cloud_update_archive_py(
        &self,
        data: Serde<UpdateArchiveParams>,
        py: Python,
    ) -> PyResult<Serde<UpdateArchiveResponse>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.cloud_update_archive(data.0)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn cloud_rename_workspace_py(
        &self,
        data: Serde<RenameWorkspaceRequest>,
        py: Python,
    ) -> PyResult<Serde<RenameWorkspaceResponse>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.cloud_rename_workspace(data.0)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn cloud_smartlog_by_version_py(
        &self,
        data: Serde<GetSmartlogByVersionParams>,
        py: Python,
    ) -> PyResult<Serde<SmartlogDataResponse>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.cloud_smartlog_by_version(data.0)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn cloud_historical_versions_py(
        &self,
        data: Serde<HistoricalVersionsParams>,
        py: Python,
    ) -> PyResult<Serde<HistoricalVersionsResponse>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.cloud_historical_versions(data.0)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn cloud_rollback_workspace_py(
        &self,
        data: Serde<RollbackWorkspaceRequest>,
        py: Python,
    ) -> PyResult<Serde<RollbackWorkspaceResponse>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.cloud_rollback_workspace(data.0)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }
}

impl<T: SaplingRemoteApi + ?Sized> SaplingRemoteApiPyExt for T {}

async fn write_trees(
    mut response: Response<Result<TreeEntry, SaplingRemoteApiServerError>>,
    store: Arc<dyn HgIdMutableDeltaStore>,
    prog: Arc<ProgressBar>,
) -> Result<Stats, SaplingRemoteApiError> {
    while let Some(Ok(entry)) = response.entries.try_next().await? {
        store.add_tree(&entry)?;
        prog.increase_position(1);
    }
    response.stats.await
}
