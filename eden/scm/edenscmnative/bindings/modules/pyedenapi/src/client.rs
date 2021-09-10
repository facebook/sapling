/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;

use cpython_async::PyFuture;
use cpython_async::TStream;
use cpython_ext::convert::Serde;
use cpython_ext::PyCell;
use cpython_ext::{ExtractInner, ExtractInnerRef, PyPathBuf, ResultPyErrExt};
use edenapi::{Builder, EdenApi};
use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitKnownResponse;
use edenapi_types::TreeEntry;
use edenapi_types::{
    CommitHashLookupResponse, CommitHashToLocationResponse, CommitLocationToHashResponse,
    CommitRevlogData, FetchSnapshotRequest, FetchSnapshotResponse, HgChangesetContent,
    HgMutationEntryContent, LookupResponse, SnapshotRawData, UploadSnapshotResponse, UploadToken,
    UploadTokensResponse, UploadTreeResponse,
};
use progress::{NullProgressFactory, ProgressFactory};
use pyconfigparser::config;
use pyprogress::PyProgressFactory;
use pyrevisionstore::{edenapifilestore, edenapitreestore};
use revisionstore::{EdenApiFileStore, EdenApiTreeStore};
use types::RepoPathBuf;

use crate::pyext::EdenApiPyExt;
use crate::stats::stats;

// Python wrapper around an EdenAPI client.
//
// This is basically just FFI boilerplate. The actual functionality
// is implemented as the default implementations of the methods in
// the `EdenApiPyExt` trait.
py_class!(pub class client |py| {
    data inner: Arc<dyn EdenApi>;
    data progress: Arc<dyn ProgressFactory>;

    def __new__(
        _cls,
        config: config,
        ui: Option<PyObject> = None,
        correlator: Option<String> = None
    ) -> PyResult<client> {
        let config = config.get_cfg(py);
        let inner = Builder::from_config(&config)
            .map_pyerr(py)?
            .correlator(correlator)
            .build()
            .map_pyerr(py)?;

        let progress = match ui {
            Some(ui) => PyProgressFactory::arc(py, ui)?,
            None => NullProgressFactory::arc(),
        };

        client::create_instance(py, inner, progress)
    }

    def health(&self) -> PyResult<PyDict> {
        self.inner(py).clone().health_py(py)
    }

    def files(
        &self,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        callback: Option<PyObject> = None
    ) -> PyResult<stats> {
        let progress = self.progress(py).clone();
        self.inner(py).clone().files_py(py, store, repo, keys, callback, progress)
    }

    def history(
        &self,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        length: Option<u32> = None,
        callback: Option<PyObject> = None
    ) -> PyResult<stats> {
        let progress = self.progress(py).clone();
        self.inner(py).clone().history_py(py, store, repo, keys, length, callback, progress)
    }

    def storetrees(
        &self,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        attributes: Option<PyDict> = None,
        callback: Option<PyObject> = None
    ) -> PyResult<stats> {
        let progress = self.progress(py).clone();
        self.inner(py).clone().storetrees_py(py, store, repo, keys, attributes, callback, progress)
    }

    def trees(
        &self,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        attributes: Option<PyDict> = None,
    ) -> PyResult<(TStream<anyhow::Result<Serde<TreeEntry>>>, PyFuture)> {
        self.inner(py).clone().trees_py(py, repo, keys, attributes)
    }

    def complete_trees(
        &self,
        store: PyObject,
        repo: String,
        rootdir: PyPathBuf,
        mfnodes: Vec<PyBytes>,
        basemfnodes: Vec<PyBytes>,
        depth: Option<usize> = None,
        callback: Option<PyObject> = None
    )  -> PyResult<stats> {
        let progress = self.progress(py).clone();
        self.inner(py).clone().complete_trees_py(
            py,
            store,
            repo,
            rootdir,
            mfnodes,
            basemfnodes,
            depth,
            callback,
            progress,
        )
    }


    /// commitdata(repo: str, nodes: [bytes], progress=None) -> [(node: bytes, data: bytes)], stats
    ///
    /// Fetch commit data in raw HG format (sorted([p1, p2]) + text).
    def commitdata(
        &self,
        repo: String,
        nodes: Vec<PyBytes>,
        callback: Option<PyObject> = None
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitRevlogData>>>, PyFuture)> {
        self.inner(py).clone().commit_revlog_data_py(py, repo, nodes, callback)
    }

    def bookmarks(
        &self,
        repo: String,
        bookmarks: Vec<String>,
        callback: Option<PyObject> = None
    ) -> PyResult<(PyDict, stats)> {
        self.inner(py).clone().bookmarks_py(py, repo, bookmarks, callback)
    }

    def hashlookup(
        &self,
        repo: String,
        hash_prefixes: Vec<String>
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitHashLookupResponse>>>, PyFuture)> {
        self.inner(py).clone().hash_lookup_py(py, repo, hash_prefixes)
    }

    def filestore(
        &self,
        repo: String
    ) -> PyResult<edenapifilestore> {
        let edenapi = self.extract_inner(py);
        let progress = Some(self.progress(py).clone());
        let store = EdenApiFileStore::new(repo, edenapi, progress);

        edenapifilestore::new(py, store)
    }

    def treestore(
        &self,
        repo: String
    ) -> PyResult<edenapitreestore> {
        let edenapi = self.extract_inner(py);
        let progress = Some(self.progress(py).clone());
        let store = EdenApiTreeStore::new(repo, edenapi, progress);

        edenapitreestore::new(py, store)
    }

    /// commitlocationtohash(repo: str, requests: [(bytes, u64, u64), progress = None) ->
    ///   [(location: (descendant: bytes, distance: u64), count: u64, hgids: [bytes])], stats
    ///
    /// Fetch the hash(es) of a location in the commit graph.
    /// A request is a tuple (descendant, distance, count) where descendant is a known hgid,
    /// distance represents how many parents we traverse from descendant to the desired commit and
    /// count represents the number of ancestors from the location that we want.
    def commitlocationtohash(
        &self,
        repo: String,
        requests: Vec<(PyBytes, u64, u64)>,
        callback: Option<PyObject> = None
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitLocationToHashResponse>>>, PyFuture)> {
        self.inner(py).clone().commit_location_to_hash_py(py, repo, requests, callback)
    }

    /// commithashtolocation(repo: str, master_heads: [bytes], hghds: [bytes], progress = None) ->
    ///   [(hgid: bytes, location: (descendant: bytes, distance: u64))], stats
    ///
    /// Fetch the location in the commit graph of a given hash.
    /// WARNING. Only hashes of ancestors of master are supported.
    /// It is necessary pass in the hashes that Segmented Changelog tracks in the master group in
    /// order for the server to be able to construct a valid location for the client.
    /// Hashes that cannot be found will be missing from the returned list.
    def commithashtolocation(
        &self,
        repo: String,
        master_heads: Vec<PyBytes>,
        hgids: Vec<PyBytes>,
        callback: Option<PyObject> = None
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitHashToLocationResponse>>>, PyFuture)> {
        self.inner(py).clone().commit_hash_to_location_py(py, repo, master_heads, hgids, callback)
    }

    /// commitknown(repo: str, nodes: [bytes]) -> [{'hgid': bytes, 'known': Result[bool]}]
    def commitknown(&self, repo: String, hgids: Vec<PyBytes>)
        -> PyResult<(TStream<anyhow::Result<Serde<CommitKnownResponse>>>, PyFuture)>
    {
        self.inner(py).clone().commit_known_py(py, repo, hgids)
    }

    /// commitgraph(repo: str, heads: [bytes], common: [bytes]) -> [{'hgid': bytes, 'parents': [bytes]}]
    def commitgraph(&self, repo: String, heads: Vec<PyBytes>, common: Vec<PyBytes>)
        -> PyResult<(TStream<anyhow::Result<Serde<CommitGraphEntry>>>, PyFuture)>
    {
        self.inner(py).clone().commit_graph_py(py, repo, heads, common)
    }

    /// clonedata(repo: str) -> bytes
    /// mincode-serialized CloneData.
    def clonedata(&self, repo: String) -> PyResult<PyBytes> {
        self.inner(py).clone().clone_data_py(py, repo)
    }

    /// pullfastforwardmaster(repo: str, old_master: Bytes, new_master: Bytes) -> PyCell
    def pullfastforwardmaster(&self, repo: String, old_master: PyBytes, new_master: PyBytes)
        -> PyResult<PyCell>
    {
        self.inner(py).clone().pull_fast_forward_master_py(py, repo, old_master, new_master)
    }


    /// lookup_file_contents(repo: str, content_ids: [bytes])
    def lookup_file_contents(&self, repo: String, content_ids: Vec<PyBytes>)
        -> PyResult<(TStream<anyhow::Result<Serde<LookupResponse>>>, PyFuture)>
    {
        self.inner(py).clone().lookup_file_contents(py, repo, content_ids)
    }

    /// lookup_commits(repo: str, nodes: [bytes])
    def lookup_commits(&self, repo: String, nodes: Vec<PyBytes>)
        -> PyResult<(TStream<anyhow::Result<Serde<LookupResponse>>>, PyFuture)>
    {
        self.inner(py).clone().lookup_commits(py, repo, nodes)
    }

    /// lookup_filenodes(repo: str, filenodes: [bytes])
    def lookup_filenodes(&self, repo: String, hgids: Vec<PyBytes>)
        -> PyResult<(TStream<anyhow::Result<Serde<LookupResponse>>>, PyFuture)>
    {
        self.inner(py).clone().lookup_filenodes(py, repo, hgids)
    }

    /// lookup_trees(repo: str, trees: [bytes])
    def lookup_trees(&self, repo: String, hgids: Vec<PyBytes>)
        -> PyResult<(TStream<anyhow::Result<Serde<LookupResponse>>>, PyFuture)>
    {
        self.inner(py).clone().lookup_trees(py, repo, hgids)
    }

    /// Upload file contents only
    def uploadfileblobs(
        &self,
        store: PyObject,
        repo: String,
        keys: Vec<(
            PyPathBuf, /* path */
            PyBytes,   /* hgid */
        )>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadTokensResponse>>>, PyFuture)> {
        self.inner(py).clone().uploadfileblobs_py(py, store, repo, keys)
    }

    /// Upload file contents and hg filenodes
    def uploadfiles(
        &self,
        store: PyObject,
        repo: String,
        keys: Vec<(
            PyPathBuf, /* path */
            PyBytes,   /* hgid */
            PyBytes,   /* p1 */
            PyBytes,   /* p2 */
        )>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadTokensResponse>>>, PyFuture)> {
        self.inner(py).clone().uploadfiles_py(py, store, repo, keys)
    }

    /// Upload trees
    def uploadtrees(
        &self,
        repo: String,
        items: Vec<(
            PyBytes,   /* hgid */
            PyBytes,   /* p1 */
            PyBytes,   /* p2 */
            PyBytes,   /* data */
        )>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadTreeResponse>>>, PyFuture)> {
        self.inner(py).clone().uploadtrees_py(py, repo, items)
    }

    /// Upload changesets
    /// This method sends a single request, batching should be done outside.
    def uploadchangesets(
        &self,
        repo: String,
        changesets: Vec<(
            PyBytes,                   /* hgid (node_id) */
            Serde<HgChangesetContent>  /* changeset content */
        )>,
        mutations: Vec<Serde<HgMutationEntryContent>>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadTokensResponse>>>, PyFuture)> {
        self.inner(py).clone().uploadchangesets_py(py, repo, changesets, mutations)
    }

    def uploadsnapshot(
        &self,
        repo: String,
        data: Serde<SnapshotRawData>,
    ) -> PyResult<Serde<UploadSnapshotResponse>> {
        self.inner(py).clone().uploadsnapshot_py(py, repo, data)
    }

    /// Fetch snapshot information
    def fetchsnapshot(
        &self,
        repo: String,
        data: Serde<FetchSnapshotRequest>,
    ) -> PyResult<Serde<FetchSnapshotResponse>> {
        self.inner(py).clone().fetchsnapshot_py(py, repo, data)
    }

    /// Downloads files from given upload tokens to given paths
    def downloadfiles(
        &self,
        repo: String,
        root: Serde<RepoPathBuf>,
        // (path to download, content id)
        files: Vec<(PyPathBuf, Serde<UploadToken>)>,
    ) -> PyResult<bool> {
        self.inner(py).clone().downloadfiles_py(py, repo, root, files)
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
        let progress = NullProgressFactory::arc();
        Self::create_instance(py, client, progress)
    }
}
