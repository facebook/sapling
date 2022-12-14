/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;

use async_runtime::block_unless_interrupted;
use cpython::*;
use cpython_async::PyFuture;
use cpython_async::TStream;
use cpython_ext::convert::Serde;
use cpython_ext::ExtractInner;
use cpython_ext::ExtractInnerRef;
use cpython_ext::PyCell;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use edenapi::Builder;
use edenapi::EdenApi;
use edenapi_ext::check_files;
use edenapi_ext::download_files;
use edenapi_ext::upload_snapshot;
use edenapi_types::AlterSnapshotRequest;
use edenapi_types::AlterSnapshotResponse;
use edenapi_types::AnyFileContentId;
use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitHashLookupResponse;
use edenapi_types::CommitHashToLocationResponse;
use edenapi_types::CommitId;
use edenapi_types::CommitIdScheme;
use edenapi_types::CommitKnownResponse;
use edenapi_types::CommitLocationToHashResponse;
use edenapi_types::CommitRevlogData;
use edenapi_types::CommitTranslateIdResponse;
use edenapi_types::EphemeralPrepareResponse;
use edenapi_types::FetchSnapshotRequest;
use edenapi_types::FetchSnapshotResponse;
use edenapi_types::FileResponse;
use edenapi_types::FileSpec;
use edenapi_types::FileType;
use edenapi_types::HgChangesetContent;
use edenapi_types::HgMutationEntryContent;
use edenapi_types::HistoryEntry;
use edenapi_types::LandStackResponse;
use edenapi_types::SnapshotRawData;
use edenapi_types::TreeAttributes;
use edenapi_types::TreeEntry;
use edenapi_types::UploadSnapshotResponse;
use edenapi_types::UploadToken;
use futures::TryStreamExt;
use minibytes::Bytes;
use pyconfigloader::config;
use pyrevisionstore::edenapifilestore;
use pyrevisionstore::edenapitreestore;
use revisionstore::EdenApiFileStore;
use revisionstore::EdenApiTreeStore;
use types::HgId;
use types::RepoPathBuf;

use crate::pyext::EdenApiPyExt;
use crate::stats::stats;
use crate::util::to_path;

// Python wrapper around an EdenAPI client.
//
// This is basically just FFI boilerplate. The actual functionality
// is implemented as the default implementations of the methods in
// the `EdenApiPyExt` trait.
py_class!(pub class client |py| {
    data inner: Arc<dyn EdenApi>;

    def __new__(
        _cls,
        config: config,
        correlator: Option<String> = None
    ) -> PyResult<client> {
        let config = config.get_cfg(py);
        let inner = Builder::from_config(&config)
            .map_pyerr(py)?
            .correlator(correlator)
            .build()
            .map_pyerr(py)?;

        client::create_instance(py, inner)
    }

    /// The URL that is intended to match `paths.default`.
    def url(&self) -> PyResult<Option<String>> {
        Ok(self.inner(py).url())
    }

    def health(&self) -> PyResult<PyDict> {
        self.inner(py).as_ref().health_py(py)
    }

    def capabilities(&self) -> PyResult<Vec<String>> {
        let client = self.inner(py).as_ref();
        let caps = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    client.capabilities().await
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(caps)
    }

    def files(
        &self,
        keys: Vec<(PyPathBuf, Serde<HgId>)>
    ) -> PyResult<TStream<anyhow::Result<Serde<FileResponse>>>> {
        self.inner(py).as_ref().files_py(py, keys)
    }

    def filesattrs(
        &self,
        spec: Serde<Vec<FileSpec>>,
    ) -> PyResult<TStream<anyhow::Result<Serde<FileResponse>>>> {
        let api = self.inner(py).as_ref();
        let entries = py
            .allow_threads(|| block_unless_interrupted(api.files_attrs(spec.0)))
            .map_pyerr(py)?
            .map_pyerr(py)?
            .entries;
        Ok(entries.map_ok(Serde).map_err(Into::into).into())
    }

    def history(
        &self,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
        length: Option<u32> = None
    ) -> PyResult<TStream<anyhow::Result<Serde<HistoryEntry>>>> {
        self.inner(py).as_ref().history_py(py, keys, length)
    }

    def storetrees(
        &self,
        store: PyObject,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
        attributes: Option<Serde<TreeAttributes>> = None
    ) -> PyResult<stats> {
        self.inner(py).as_ref().storetrees_py(py, store, keys, attributes.map(|a| a.0))
    }

    def trees(
        &self,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
        attributes: Option<Serde<TreeAttributes>> = None
    ) -> PyResult<(TStream<anyhow::Result<Serde<TreeEntry>>>, PyFuture)> {
        self.inner(py).as_ref().trees_py(py, keys, attributes.map(|a| a.0))
    }

    /// commitdata(nodes: [bytes]) -> [(node: bytes, data: bytes)], stats
    ///
    /// Fetch commit data in raw HG format (sorted([p1, p2]) + text).
    def commitdata(
        &self,
        nodes: Serde<Vec<HgId>>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitRevlogData>>>, PyFuture)> {
        self.inner(py).as_ref().commit_revlog_data_py(py, nodes.0)
    }

    /// bookmarks([name]) -> {name: node|None}
    ///
    /// Resolve remote bookmarks.
    def bookmarks(
        &self,
        bookmarks: Vec<String>
    ) -> PyResult<PyDict> {
        self.inner(py).as_ref().bookmarks_py(py, bookmarks)
    }

    /// setbookmark(bookmark, to, from, pushvars)
    ///
    /// Create, delete, or move a bookmark.
    def setbookmark(
        &self,
        bookmark: String,
        to: Serde<Option<HgId>>,
        from: Serde<Option<HgId>>,
        pushvars: Vec<(String, String)> = Vec::new(),
    ) -> PyResult<bool> {
        self.inner(py).as_ref().set_bookmark_py(py, bookmark, to.0, from.0, pushvars)
    }

    /// land_stack(bookmark, head, base, pushvars) -> {old_hgids: [node], new_hgids: [node]}
    ///
    /// Land a stack of already-uploaded commits by rebasing onto a bookmark and updating the bookmark.
    def landstack(
        &self,
        bookmark: String,
        head: Serde<HgId>,
        base: Serde<HgId>,
        pushvars: Vec<(String, String)> = Vec::new(),
    ) -> PyResult<Serde<LandStackResponse>> {
        self.inner(py).as_ref().land_stack_py(py, bookmark, head.0, base.0, pushvars)
    }

    /// hashlookup(hexprefix) -> [{'request': {'InclusiveRange': (start_node, end_node)},
    ///                            'hgids': [node]}]
    ///
    /// Lookup commit hashes by hex prefixes.
    def hashlookup(
        &self,
        hash_prefixes: Vec<String>
    ) -> PyResult<Serde<Vec<CommitHashLookupResponse>>> {
        self.inner(py).as_ref().hash_lookup_py(py, hash_prefixes)
    }

    def filestore(
        &self
    ) -> PyResult<edenapifilestore> {
        let edenapi = self.extract_inner(py);
        let store = EdenApiFileStore::new(edenapi);

        edenapifilestore::new(py, store)
    }

    def treestore(
        &self
    ) -> PyResult<edenapitreestore> {
        let edenapi = self.extract_inner(py);
        let store = EdenApiTreeStore::new(edenapi);

        edenapitreestore::new(py, store)
    }

    /// commitlocationtohash(requests: [(bytes, u64, u64)) ->
    ///   [(location: (descendant: bytes, distance: u64), count: u64, hgids: [bytes])]
    ///
    /// Fetch the hash(es) of a location in the commit graph.
    /// A request is a tuple (descendant, distance, count) where descendant is a known hgid,
    /// distance represents how many parents we traverse from descendant to the desired commit and
    /// count represents the number of ancestors from the location that we want.
    def commitlocationtohash(
        &self,
        requests: Serde<Vec<(HgId, u64, u64)>>,
    ) -> PyResult<Serde<Vec<CommitLocationToHashResponse>>> {
        self.inner(py).as_ref().commit_location_to_hash_py(py, requests.0)
    }

    /// commithashtolocation(master_heads: [bytes], hghds: [bytes]) ->
    ///   [(hgid: bytes, location: (descendant: bytes, distance: u64))]
    ///
    /// Fetch the location in the commit graph of a given hash.
    /// WARNING. Only hashes of ancestors of master are supported.
    /// It is necessary pass in the hashes that Segmented Changelog tracks in the master group in
    /// order for the server to be able to construct a valid location for the client.
    /// Hashes that cannot be found will be missing from the returned list.
    def commithashtolocation(
        &self,
        master_heads: Serde<Vec<HgId>>,
        hgids: Serde<Vec<HgId>>,
    ) -> PyResult<Serde<Vec<CommitHashToLocationResponse>>> {
        self.inner(py).as_ref().commit_hash_to_location_py(py, master_heads.0, hgids.0)
    }

    /// commitknown(nodes: [bytes]) -> [{'hgid': bytes, 'known': Result[bool]}]
    def commitknown(&self, hgids: Serde<Vec<HgId>>)
        -> PyResult<Serde<Vec<CommitKnownResponse>>>
    {
        self.inner(py).as_ref().commit_known_py(py, hgids.0)
    }

    /// commitgraph(heads: [bytes], common: [bytes]) -> [{'hgid': bytes, 'parents': [bytes]}]
    def commitgraph(&self, heads: Serde<Vec<HgId>>, common: Serde<Vec<HgId>>)
        -> PyResult<Serde<Vec<CommitGraphEntry>>>
    {
        self.inner(py).as_ref().commit_graph_py(py, heads.0, common.0)
    }

    /// clonedata() -> PyCell
    def clonedata(&self) -> PyResult<PyCell> {
        self.inner(py).as_ref().clone_data_py(py)
    }

    /// pullfastforwardmaster(old_master: Bytes, new_master: Bytes) -> PyCell
    def pullfastforwardmaster(&self, old_master: Serde<HgId>, new_master: Serde<HgId>)
        -> PyResult<PyCell>
    {
        self.inner(py).as_ref().pull_fast_forward_master_py(py, old_master.0, new_master.0)
    }

    /// pulllazy(common: [node], missing: [node]) -> PyCell
    def pulllazy(&self, common: Serde<Vec<HgId>>, missing: Serde<Vec<HgId>>) -> PyResult<PyCell> {
        self.inner(py).as_ref().pull_lazy_py(py, common.0, missing.0)
    }

    /// lookup_file_contents(content_ids: [bytes])
    def lookup_file_contents(&self, content_ids: Vec<PyBytes>)
        -> PyResult<Serde<Vec<(usize, UploadToken)>>>
    {
        self.inner(py).as_ref().lookup_file_contents(py, content_ids)
    }

    /// lookup_commits(nodes: [bytes])
    def lookup_commits(&self, nodes: Serde<Vec<HgId>>)
        -> PyResult<Serde<Vec<(usize, UploadToken)>>>
    {
        self.inner(py).as_ref().lookup_commits(py, nodes.0)
    }

    /// lookup_filenodes(filenodes: [bytes])
    def lookup_filenodes(&self, hgids: Serde<Vec<HgId>>)
        -> PyResult<Serde<Vec<(usize, UploadToken)>>>
    {
        self.inner(py).as_ref().lookup_filenodes(py, hgids.0)
    }

    /// lookup_trees(trees: [bytes])
    def lookup_trees(&self, hgids: Serde<Vec<HgId>>)
        -> PyResult<Serde<Vec<(usize, UploadToken)>>>
    {
        self.inner(py).as_ref().lookup_trees(py, hgids.0)
    }

    /// lookup_filenodes_and_trees(filenodes: [bytes], trees: [bytes])
    def lookup_filenodes_and_trees(&self, filenodes: Serde<Vec<HgId>>, trees: Serde<Vec<HgId>>)
        -> PyResult<Serde<Vec<(usize, UploadToken)>>>
    {
        self.inner(py).as_ref().lookup_filenodes_and_trees(py, filenodes.0, trees.0)
    }

    /// Upload file contents only
    def uploadfileblobs(
        &self,
        store: PyObject,
        keys: Vec<(
            PyPathBuf,   /* path */
            Serde<HgId>, /* hgid */
        )>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        self.inner(py).as_ref().uploadfileblobs_py(py, store, keys)
    }

    /// Upload file contents and hg filenodes
    def uploadfiles(
        &self,
        store: PyObject,
        keys: Vec<(
            PyPathBuf,     /* path */
            Serde<HgId>,   /* hgid */
            Serde<HgId>,   /* p1 */
            Serde<HgId>,   /* p2 */
        )>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        self.inner(py).as_ref().uploadfiles_py(py, store, keys)
    }

    /// Upload trees
    def uploadtrees(
        &self,
        items: Vec<(
            Serde<HgId>,  /* hgid */
            Serde<HgId>,  /* p1 */
            Serde<HgId>,  /* p2 */
            PyBytes,      /* data */
        )>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        self.inner(py).as_ref().uploadtrees_py(py, items)
    }

    /// Upload changesets
    /// This method sends a single request, batching should be done outside.
    def uploadchangesets(
        &self,
        changesets: Serde<Vec<(
            HgId,               /* hgid (node_id) */
            HgChangesetContent  /* changeset content */
        )>>,
        mutations: Vec<Serde<HgMutationEntryContent>>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        self.inner(py).as_ref().uploadchangesets_py(py, changesets.0, mutations)
    }

    def uploadsnapshot(
        &self,
        data: Serde<SnapshotRawData>,
        custom_duration_secs: Option<u64>,
        copy_from_bubble_id: Option<u64>,
        use_bubble: Option<u64>,
        labels: Option<Vec<String>>,
    ) -> PyResult<Serde<UploadSnapshotResponse>> {
        let api = self.inner(py).as_ref();
        let copy_from_bubble_id = copy_from_bubble_id.and_then(NonZeroU64::new);
        let use_bubble = use_bubble.and_then(NonZeroU64::new);
        py.allow_threads(|| {
            block_unless_interrupted(upload_snapshot(
                api,
                data.0,
                custom_duration_secs,
                copy_from_bubble_id,
                use_bubble,
                labels,
            ))
        })
        .map_pyerr(py)?
        .map_pyerr(py)
        .map(Serde)
    }

    /// Fetch snapshot information
    def fetchsnapshot(
        &self,
        data: Serde<FetchSnapshotRequest>,
    ) -> PyResult<Serde<FetchSnapshotResponse>> {
        self.inner(py).as_ref().fetchsnapshot_py(py, data)
    }

    /// Alter the properties of an existing snapshot
    def altersnapshot(
        &self,
        data: Serde<AlterSnapshotRequest>,
    ) -> PyResult<Serde<AlterSnapshotResponse>> {
        self.inner(py).as_ref().altersnapshot_py(py, data)
    }

    /// Downloads files from given upload tokens to given paths
    def downloadfiles(
        &self,
        root: Serde<RepoPathBuf>,
        // (path to download, content id)
        files: Vec<(PyPathBuf, Serde<UploadToken>, Serde<FileType>)>,
    ) -> PyResult<bool> {
        let api = self.inner(py).as_ref();
        let files = files
            .into_iter()
            .map(|(p, token, tp)| Ok((to_path(py, &p)?, token.0, tp.0)))
            .collect::<Result<Vec<_>, PyErr>>()?;
        py.allow_threads(|| block_unless_interrupted(download_files(api, &root.0, files)))
            .map_pyerr(py)?
            .map_pyerr(py)
            .map(|_| true)
    }

    /// Checks which files differ from
    def checkfiles(
        &self,
        root: Serde<RepoPathBuf>,
        files: Vec<(PyPathBuf, Serde<UploadToken>, Serde<FileType>)>
    ) -> PyResult<Vec<PyPathBuf>> {
        let files = files
            .into_iter()
            .map(|(p, token, tp)| Ok((to_path(py, &p)?, token.0, tp.0)))
            .collect::<Result<Vec<_>, PyErr>>()?;
        py.allow_threads(|| block_unless_interrupted(check_files(&root.0, files)))
            .map_pyerr(py)?
            .map_pyerr(py)
            .map(|v| v.into_iter().map(Into::into).collect())
    }

    /// Download file from given upload token to memory
    def downloadfiletomemory(
        &self,
        token: Serde<UploadToken>
    ) -> PyResult<PyBytes> {
        self.inner(py).as_ref().downloadfiletomemory_py(py, token)
    }

    def ephemeralprepare(&self, custom_duration: Option<u64>, labels: Option<Vec<String>>)
        -> PyResult<TStream<anyhow::Result<Serde<EphemeralPrepareResponse>>>>
    {
        let api = self.inner(py).as_ref();
        let entries = py
            .allow_threads(|| block_unless_interrupted(api.ephemeral_prepare(custom_duration.map(Duration::from_secs), labels)))
            .map_pyerr(py)?
            .map_pyerr(py)?
            .entries;
        Ok(entries.map_ok(Serde).map_err(Into::into).into())
    }

    /// uploadfilecontents(data: [(AnyFileContentId, Bytes)], bubbleid: int | None)
    /// -> Iterable[UploadToken]
    def uploadfilecontents(
        &self,
        data: Serde<Vec<(AnyFileContentId, Bytes)>>,
        bubbleid: Option<u64> = None
    ) -> PyResult<TStream<anyhow::Result<Serde<UploadToken>>>> {
        let api = self.inner(py).as_ref();
        let bubble_id = bubbleid.and_then(NonZeroU64::new);
        let entries = py
            .allow_threads(|| block_unless_interrupted(api.process_files_upload(data.0, bubble_id, None)))
            .map_pyerr(py)?
            .map_pyerr(py)?
            .entries;
        Ok(entries.map_ok(Serde).map_err(Into::into).into())
    }

    def commitmutations(
        &self,
        commits: Serde<Vec<HgId>>,
    ) -> PyResult<Serde<Vec<HgMutationEntryContent>>> {
        let api = self.inner(py).as_ref();
        py.allow_threads(|| block_unless_interrupted(api.commit_mutations(commits.0)))
            .map_pyerr(py)?
            .map_pyerr(py)
            .map(|responses| Serde(responses.into_iter().map(|r| r.mutation).collect()))
    }

    def committranslateids(
        &self,
        commits: Serde<Vec<CommitId>>,
        scheme: Serde<CommitIdScheme>,
    ) -> PyResult<TStream<anyhow::Result<Serde<CommitTranslateIdResponse>>>> {
        let api = self.inner(py).as_ref();
        let responses = py.allow_threads(|| block_unless_interrupted(api.commit_translate_id(commits.0, scheme.0)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(responses.entries.map_ok(Serde).map_err(Into::into).into())
    }
});

impl ExtractInnerRef for client {
    type Inner = Arc<dyn EdenApi>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.inner(py)
    }
}

impl client {
    pub fn from_edenapi(py: Python, client: Arc<dyn EdenApi>) -> PyResult<Self> {
        Self::create_instance(py, client)
    }
}
