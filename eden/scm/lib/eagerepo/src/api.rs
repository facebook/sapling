/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Write;
use std::num::NonZeroU64;
use std::sync::Arc;

use anyhow::anyhow;
use blob::Blob;
use configmodel::Config;
use configmodel::ConfigExt;
use dag::Location;
use dag::Set;
use dag::Vertex;
use dag::ops::DagAlgorithm;
use dag::ops::DagExportPullData;
use dag::ops::PrefixLookup;
use dag::protocol::AncestorPath;
use dag::protocol::RemoteIdConvertProtocol;
use edenapi::Response;
use edenapi::ResponseMeta;
use edenapi::SaplingRemoteApi;
use edenapi::SaplingRemoteApiError;
use edenapi::api::UploadLookupPolicy;
use edenapi::configmodel;
use edenapi::types::AnyFileContentId;
use edenapi::types::AnyId;
use edenapi::types::BookmarkEntry;
use edenapi::types::CommitGraphEntry;
use edenapi::types::CommitGraphSegments;
use edenapi::types::CommitGraphSegmentsEntry;
use edenapi::types::CommitHashLookupResponse;
use edenapi::types::CommitHashToLocationResponse;
use edenapi::types::CommitId;
use edenapi::types::CommitIdScheme;
use edenapi::types::CommitKnownResponse;
use edenapi::types::CommitLocationToHashRequest;
use edenapi::types::CommitLocationToHashResponse;
use edenapi::types::CommitMutationsResponse;
use edenapi::types::CommitRevlogData;
use edenapi::types::CommitTranslateIdResponse;
use edenapi::types::Extra;
use edenapi::types::FileAuxData;
use edenapi::types::FileContent;
use edenapi::types::FileEntry;
use edenapi::types::FileMetadata;
use edenapi::types::FileResponse;
use edenapi::types::FileSpec;
use edenapi::types::HgChangesetContent;
use edenapi::types::HgFilenodeData;
use edenapi::types::HgId;
use edenapi::types::HgMutationEntryContent;
use edenapi::types::HistoryEntry;
use edenapi::types::IndexableId;
use edenapi::types::Key;
use edenapi::types::LandStackData;
use edenapi::types::LandStackResponse;
use edenapi::types::LookupResponse;
use edenapi::types::LookupResult;
use edenapi::types::NodeInfo;
use edenapi::types::Parents;
use edenapi::types::RepoPathBuf;
use edenapi::types::SaplingRemoteApiServerError;
use edenapi::types::ServerError;
use edenapi::types::SetBookmarkResponse;
use edenapi::types::SuffixQueryResponse;
use edenapi::types::TreeAttributes;
use edenapi::types::TreeChildEntry;
use edenapi::types::TreeChildFileEntry;
use edenapi::types::TreeEntry;
use edenapi::types::UploadHgChangeset;
use edenapi::types::UploadToken;
use edenapi::types::UploadTokenData;
use edenapi::types::UploadTokensResponse;
use edenapi::types::UploadTreeEntry;
use edenapi::types::UploadTreeResponse;
use edenapi::types::make_hash_lookup_request;
use edenapi_trait as edenapi;
use edenapi_types::bookmark::Freshness;
use format_util::git_sha1_deserialize;
use format_util::hg_sha1_deserialize;
use futures::StreamExt;
use futures::stream::BoxStream;
use futures::stream::TryStreamExt;
use http::StatusCode;
use http::Version;
use manifest::DiffType;
use manifest::Manifest;
use manifest_augmented_tree::AugmentedTreeWithDigest;
use manifest_tree::Flag;
use manifest_tree::TreeManifest;
use minibytes::Bytes;
use mutationstore::MutationEntry;
use nonblocking::non_blocking_result;
use pathmatcher::AlwaysMatcher;
use repourl::RepoUrl;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::Kind;
use storemodel::SerializationFormat;
use storemodel::types::FetchContext;
use tracing::debug;
use tracing::error;
use tracing::trace;

use crate::EagerRepo;

impl EagerRepo {
    /// Load file/tree store changes from disk.
    ///
    /// This is intended to be used by SaplingRemoteApi impls so content fetched
    /// via SaplingRemoteApi (during testing) is always fresh.
    pub(crate) fn refresh_for_api(&self) {
        let _ = self.store.flush();
    }
}

#[async_trait::async_trait]
impl SaplingRemoteApi for EagerRepo {
    fn url(&self) -> Option<String> {
        Some(format!("eager:{}", self.dir.display()))
    }

    async fn health(&self) -> edenapi::Result<ResponseMeta> {
        Ok(default_response_meta())
    }

    async fn capabilities(&self) -> Result<Vec<String>, SaplingRemoteApiError> {
        let mut caps = vec![
            "segmented-changelog".to_string(),
            "commit-graph-segments".to_string(),
            // Inform client that we only support sha1 content addressing.
            "sha1-only".to_string(),
            // Inform the client that we suppot most common sapling operations like files, trees, blame etc. but not commit graph segments or commit cloud
            "sapling-common".to_string(),
        ];
        if matches!(self.format(), SerializationFormat::Git) {
            caps.push("git-format".to_string());
        }
        Ok(caps)
    }

    async fn files(
        &self,
        _fctx: FetchContext,
        keys: Vec<Key>,
    ) -> edenapi::Result<Response<FileResponse>> {
        debug!("files {}", debug_key_list(&keys));
        self.refresh_for_api();
        let mut values = Vec::with_capacity(keys.len());
        for key in keys {
            let id = key.hgid;
            let data = self.get_sha1_blob_for_api(id, "files")?;

            let (parents, body) = sha1_blob_to_parents_body(&data, self.format())?;

            let entry = FileEntry {
                key: key.clone(),
                parents,
                content: Some(FileContent {
                    hg_file_blob: body,
                    metadata: Default::default(),
                }),
                aux_data: None,
            };
            let response = FileResponse {
                key,
                result: Ok(entry),
            };
            values.push(Ok(response));
        }
        Ok(convert_to_response(values))
    }

    async fn files_attrs(
        &self,
        _fctx: FetchContext,
        reqs: Vec<FileSpec>,
    ) -> edenapi::Result<Response<FileResponse>> {
        ::fail::fail_point!("eagerepo::api::files_attrs", |_| {
            Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "failpoint".to_string(),
                headers: Default::default(),
                url: self.url("files_attrs"),
            })
        });

        debug!("files_attrs {}", debug_spec_list(&reqs));
        self.refresh_for_api();
        let mut values = Vec::with_capacity(reqs.len());
        for spec in reqs {
            let key = spec.key;
            let id = key.hgid;
            let data = self.get_sha1_blob_for_api(id, "files_attrs")?;

            let (parents, body) = sha1_blob_to_parents_body(&data, self.format())?;

            let mut entry = FileEntry {
                key: key.clone(),
                parents,
                content: None,
                aux_data: None,
            };

            if spec.attrs.aux_data {
                let (pure_content, copy_from) =
                    file_body_to_file_content_and_copy_from(&body, self.format());

                let mut aux_data = FileAuxData::from_content(&Blob::Bytes(pure_content));
                aux_data.file_header_metadata = Some(copy_from);

                entry.aux_data = Some(aux_data);
            }

            if spec.attrs.content {
                entry.content = Some(FileContent {
                    hg_file_blob: body,
                    metadata: Default::default(),
                });
            }

            let response = FileResponse {
                key,
                result: Ok(entry),
            };
            values.push(Ok(response));
        }
        Ok(convert_to_response(values))
    }

    async fn history(
        &self,
        keys: Vec<Key>,
        _length: Option<u32>,
    ) -> edenapi::Result<Response<HistoryEntry>> {
        debug!("history {}", debug_key_list(&keys));
        self.refresh_for_api();
        let mut values = Vec::new();
        let mut visited: HashSet<Key> = Default::default();
        let mut to_visit: Vec<Key> = keys;
        while let Some(key) = to_visit.pop() {
            if !visited.insert(key.clone()) {
                continue;
            }

            // Don't report missing files as errors. This matches Mononoke's behavior.
            let Some(data) = self.opt_sha1_blob_for_api(key.hgid, "history")? else {
                continue;
            };

            let (parents, body) = sha1_blob_to_parents_body(&data, self.format())?;

            // NOTE: Order of p1, p2 are not preserved, unlike revlog hg.
            // It should be okay correctness-wise.
            let (p1, p2) = parents.into_nodes();
            let mut key1 = Key {
                path: key.path.clone(),
                hgid: p1,
            };
            let mut key2 = Key {
                path: key.path.clone(),
                hgid: p2,
            };
            if let Some(renamed_from) = extract_rename(&body) {
                if p1.is_null() {
                    key1 = renamed_from;
                } else {
                    key2 = renamed_from;
                }
            }
            if !p1.is_null() {
                to_visit.push(key1.clone());
            }
            if !p2.is_null() {
                to_visit.push(key2.clone());
            }
            let entry = HistoryEntry {
                key,
                nodeinfo: NodeInfo {
                    parents: [key1, key2],
                    linknode: *HgId::null_id(),
                },
            };
            values.push(Ok(entry));
        }
        Ok(convert_to_response(values))
    }

    async fn trees(
        &self,
        _fctx: FetchContext,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> edenapi::Result<Response<Result<TreeEntry, SaplingRemoteApiServerError>>> {
        debug!("trees {} {:?}", debug_key_list(&keys), attributes);

        ::fail::fail_point!("eagerepo::api::trees", |_| {
            Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "failpoint".to_string(),
                headers: Default::default(),
                url: self.url("trees"),
            })
        });

        self.refresh_for_api();
        let mut values = Vec::new();
        let attributes = attributes.unwrap_or_default();
        if attributes.augmented_trees {
            for key in keys {
                match self
                    .get_augmented_tree_blob_with_digest_for_api(key.hgid, "trees")
                    .await
                {
                    Ok(tree) => {
                        let augmented_tree_with_digest =
                            AugmentedTreeWithDigest::try_deserialize(std::io::Cursor::new(tree))?;
                        let mut converted_entry: TreeEntry =
                            TreeEntry::try_from(augmented_tree_with_digest).map_err(|err| {
                                SaplingRemoteApiServerError::with_key(key.clone(), err)
                            })?;
                        // Match the key in the response and in the request!
                        // TreeEntry produced from the augmented tree format doesn't contain path for itself, only for the children.
                        converted_entry.key = key;
                        // Clean up fields that were not requested, since the augmented trees format contains
                        // everything by default
                        if !attributes.manifest_blob {
                            converted_entry.with_data(None);
                        }
                        if !attributes.parents {
                            converted_entry.with_parents(None);
                        }
                        if !attributes.child_metadata {
                            converted_entry.with_children(None);
                        }
                        values.push(Ok(Ok(converted_entry)));
                    }
                    Err(e) => values.push(Err(e)),
                }
            }
        } else {
            for key in keys {
                let sha1_blob = self.get_sha1_blob_for_api(key.hgid, "trees")?;
                let (parents, body) = sha1_blob_to_parents_body(&sha1_blob, self.format())?;
                let mut entry = TreeEntry {
                    key: key.clone(),
                    ..Default::default()
                };

                if attributes.manifest_blob {
                    entry.data = Some(body.clone());
                }

                if attributes.parents {
                    entry.parents = Some(parents);
                }

                if attributes.child_metadata {
                    let mut children: Vec<Result<TreeChildEntry, SaplingRemoteApiServerError>> =
                        Vec::new();

                    let tree_entry = manifest_tree::TreeEntry(body, self.format());
                    for child in tree_entry.elements() {
                        let child = match child {
                            Ok(child) => child,
                            Err(err) => {
                                children.push(Err(SaplingRemoteApiServerError::with_key(
                                    key.clone(),
                                    err,
                                )));
                                continue;
                            }
                        };

                        match child.flag {
                            Flag::File(_) => {
                                let file_sha1_blob =
                                    self.get_sha1_blob_for_api(child.hgid, "trees (aux)")?;
                                let (_file_parents, file_body) =
                                    sha1_blob_to_parents_body(&file_sha1_blob, self.format())?;

                                let (file_body, copy_from) =
                                    file_body_to_file_content_and_copy_from(
                                        &file_body,
                                        self.format(),
                                    );

                                let mut aux_data =
                                    FileAuxData::from_content(&Blob::Bytes(file_body));
                                aux_data.file_header_metadata = Some(copy_from);

                                children.push(Ok(TreeChildEntry::File(TreeChildFileEntry {
                                    key: Key::new(
                                        RepoPathBuf::from_string(child.component.to_string())
                                            .map_err(anyhow::Error::from)?,
                                        child.hgid,
                                    ),
                                    file_metadata: Some(FileMetadata::from(aux_data)),
                                })));
                            }
                            // The client currently ignores directory metadata, so don't bother.
                            Flag::Directory => {}
                        }
                    }

                    entry.children = Some(children);
                }

                values.push(Ok(Ok(entry)));
            }
        }
        Ok(convert_to_response(values))
    }

    async fn commit_revlog_data(
        &self,
        hgids: Vec<HgId>,
    ) -> edenapi::Result<Response<CommitRevlogData>> {
        debug!("revlog_data {}", debug_hgid_list(&hgids));
        self.refresh_for_api();
        let mut values = Vec::new();
        for id in hgids {
            let data = self.get_sha1_blob_for_api(id, "commit_revlog_data")?;
            let data = CommitRevlogData {
                hgid: id,
                revlog_data: match self.format() {
                    SerializationFormat::Hg => data,
                    SerializationFormat::Git => {
                        // For Git, just return the commit data without hesders.
                        let git_commit_data = git_sha1_deserialize(&data)?.0;
                        data.slice_to_bytes(git_commit_data)
                    }
                },
            };
            values.push(Ok(data));
        }
        Ok(convert_to_response(values))
    }

    async fn commit_location_to_hash(
        &self,
        requests: Vec<CommitLocationToHashRequest>,
    ) -> edenapi::Result<Vec<CommitLocationToHashResponse>> {
        self.refresh_for_api();
        let path_names: Vec<(AncestorPath, Vec<Vertex>)> = {
            let paths: Vec<AncestorPath> = requests
                .into_iter()
                .map(|r| AncestorPath {
                    x: Vertex::copy_from(r.location.descendant.as_ref()),
                    n: r.location.distance,
                    batch_size: r.count,
                })
                .collect();
            self.dag()
                .await
                .resolve_relative_paths_to_names(paths)
                .await
                .map_err(map_dag_err)?
        };

        check_convert_to_hgid(path_names.iter().flat_map(|i| i.1.iter()))?;
        check_convert_to_hgid(path_names.iter().map(|i| &i.0.x))?;

        let values: edenapi::Result<Vec<CommitLocationToHashResponse>> = path_names
            .into_iter()
            .map(|(p, ns)| {
                let count = ns.len();
                let response = CommitLocationToHashResponse {
                    location: Location {
                        descendant: HgId::from_slice(p.x.as_ref()).unwrap(), // unwrap: checked above
                        distance: p.n,
                    },
                    hgids: ns
                        .into_iter()
                        .map(|n| HgId::from_slice(n.as_ref()).unwrap()) // unwrap: checked above
                        .collect(),
                    count: count as _,
                };
                Ok(response)
            })
            .collect();

        values
    }

    async fn commit_hash_to_location(
        &self,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
    ) -> edenapi::Result<Vec<CommitHashToLocationResponse>> {
        self.refresh_for_api();
        let path_names: Vec<(AncestorPath, Vec<Vertex>)> = {
            let heads: Vec<Vertex> = to_vec_vertex(&master_heads);
            let names: Vec<Vertex> = to_vec_vertex(&hgids);
            self.dag()
                .await
                .resolve_names_to_relative_paths(heads, names)
                .await
                .map_err(map_dag_err)?
        };

        check_convert_to_hgid(path_names.iter().flat_map(|i| i.1.iter()))?;
        check_convert_to_hgid(path_names.iter().map(|i| &i.0.x))?;

        let values: edenapi::Result<Vec<CommitHashToLocationResponse>> = path_names
            .into_iter()
            .flat_map(|(p, ns)| {
                ns.into_iter()
                    .enumerate()
                    .map(|(i, n)| {
                        CommitHashToLocationResponse {
                            hgid: HgId::from_slice(n.as_ref()).unwrap(), // unwrap: checked above
                            result: Ok(Some(Location {
                                descendant: HgId::from_slice(p.x.as_ref()).unwrap(), // unwrap: checked above
                                distance: p.n + (i as u64),
                            })),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .map(Ok)
            .collect();

        // For hgids outside the master group, just ignore them.
        // It's okay to return them with result "None" too.

        values
    }

    async fn commit_known(&self, hgids: Vec<HgId>) -> edenapi::Result<Vec<CommitKnownResponse>> {
        debug!("commit_known {}", debug_hgid_list(&hgids));
        self.refresh_for_api();
        let mut values = Vec::new();
        for id in hgids {
            let known = self.get_sha1_blob(id).map_err(map_crate_err)?.is_some();
            let response = CommitKnownResponse {
                hgid: id,
                known: Ok(known),
            };
            values.push(response);
        }
        Ok(values)
    }

    async fn commit_graph(
        &self,
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> Result<Vec<CommitGraphEntry>, SaplingRemoteApiError> {
        debug!(
            "commit_graph {} {}",
            debug_hgid_list(&heads),
            debug_hgid_list(&common),
        );
        self.refresh_for_api();
        let heads = to_set(&heads);
        let common = to_set(&common);
        let graph = self
            .dag()
            .await
            .only(heads, common)
            .await
            .map_err(map_dag_err)?;
        let stream = graph.iter_rev().await.map_err(map_dag_err)?;
        let stream: BoxStream<edenapi::Result<CommitGraphEntry>> = stream
            .then(|s| async move {
                let s = s?;
                let hgid = HgId::from_slice(s.as_ref()).unwrap();
                let parents = self.dag().await.parent_names(s).await?;
                let parents: Vec<HgId> = parents
                    .into_iter()
                    .map(|v| HgId::from_slice(v.as_ref()).unwrap())
                    .collect();
                let entry = CommitGraphEntry {
                    hgid,
                    parents,
                    is_draft: None,
                };
                Ok(entry)
            })
            .map_err(map_dag_err)
            .boxed();
        let values: edenapi::Result<Vec<CommitGraphEntry>> = stream.try_collect().await;
        values
    }

    async fn commit_graph_segments(
        &self,
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> Result<Vec<CommitGraphSegmentsEntry>, SaplingRemoteApiError> {
        ::fail::fail_point!("eagerepo::api::commitgraphsegments", |_| {
            Err(SaplingRemoteApiError::NotSupported)
        });

        debug!(
            "commit_graph_segments {} {}",
            debug_hgid_list(&heads),
            debug_hgid_list(&common),
        );
        self.refresh_for_api();
        let heads = to_set(&heads);
        let common = to_set(&common);
        let graph = self
            .dag()
            .await
            .only(heads, common)
            .await
            .map_err(map_dag_err)?;

        let graph_segments: CommitGraphSegments = self
            .dag()
            .await
            .export_pull_data(&graph)
            .await
            .map_err(map_dag_err)?
            .try_into()?;

        Ok(graph_segments.segments)
    }

    async fn bookmarks(
        &self,
        bookmarks: Vec<String>,
        _freshness: Option<Freshness>,
    ) -> edenapi::Result<Vec<BookmarkEntry>> {
        debug!("bookmarks {}", debug_string_list(&bookmarks));
        self.refresh_for_api();
        let mut values = Vec::new();
        let map = self.get_bookmarks_map().map_err(map_crate_err)?;
        for name in bookmarks {
            let opt_id = map.get(&name).cloned();
            let entry = BookmarkEntry {
                bookmark: name,
                hgid: opt_id,
            };
            values.push(entry);
        }
        Ok(values)
    }

    async fn set_bookmark(
        &self,
        bookmark: String,
        to: Option<HgId>,
        from: Option<HgId>,
        _pushvars: HashMap<String, String>,
    ) -> Result<SetBookmarkResponse, SaplingRemoteApiError> {
        debug!("bookmarks {:?} -> {:?}", from, to);
        self.refresh_for_api();

        let mut bms = self.get_bookmarks_map().map_err(map_crate_err)?;

        if to.is_none() && from.is_none() {
            return Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::BAD_REQUEST,
                message: "must specify one of 'to' or 'from'".to_string(),
                headers: Default::default(),
                url: self.url("set_bookmark"),
            });
        }

        if let Some(from) = from {
            match bms.get(&bookmark) {
                None => {
                    return Err(SaplingRemoteApiError::HttpError {
                        status: StatusCode::NOT_FOUND,
                        message: format!("bookmark {bookmark} doesn't exist"),
                        headers: Default::default(),
                        url: self.url("set_bookmark"),
                    });
                }
                Some(node) => {
                    if *node != from {
                        return Err(SaplingRemoteApiError::HttpError {
                            status: StatusCode::BAD_REQUEST,
                            message: format!(
                                "bookmark {bookmark}'s current value is {node}, not {from}"
                            ),
                            headers: Default::default(),
                            url: self.url("set_bookmark"),
                        });
                    }
                }
            }
        } else if bms.contains_key(&bookmark) {
            return Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::BAD_REQUEST,
                message: format!("bookmark {bookmark} already exists"),
                headers: Default::default(),
                url: self.url("set_bookmark"),
            });
        }

        match to {
            None => bms.remove(&bookmark),
            Some(to) => bms.insert(bookmark, to),
        };

        // This validates that the bookmark value is a valid commit.
        self.set_bookmarks_map(bms).map_err(map_crate_err)?;

        self.flush_for_api("set_bookmark").await?;

        Ok(SetBookmarkResponse { data: Ok(()) })
    }

    async fn hash_prefixes_lookup(
        &self,
        prefixes: Vec<String>,
    ) -> Result<Vec<CommitHashLookupResponse>, SaplingRemoteApiError> {
        self.refresh_for_api();
        let dag = self.dag().await;
        prefixes
            .into_iter()
            .map(
                move |prefix| -> Result<CommitHashLookupResponse, SaplingRemoteApiError> {
                    let req = make_hash_lookup_request(prefix.clone())?;
                    let resp =
                        non_blocking_result(dag.vertexes_by_hex_prefix(prefix.as_bytes(), 100))
                            .map_err(|e| SaplingRemoteApiError::Other(e.into()));
                    resp.and_then(|vertexes| {
                        Ok(CommitHashLookupResponse {
                            request: req,
                            hgids: vertexes
                                .into_iter()
                                .map(|vertex| {
                                    HgId::from_hex(vertex.to_hex().as_bytes())
                                        .map_err(anyhow::Error::from)
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                        })
                    })
                },
            )
            .collect()
    }

    async fn commit_mutations(
        &self,
        mut commits: Vec<HgId>,
    ) -> Result<Vec<CommitMutationsResponse>, SaplingRemoteApiError> {
        commits.sort();
        debug!("commit_mutations {}", debug_hgid_list(&commits));
        self.refresh_for_api();

        let mut seen_commits = HashSet::new();
        let mut mutations = Vec::new();
        let mut_store = self.mut_store.lock().await;

        // Max of 100 mutation depth.
        for _ in 0..100 {
            commits.retain(|c| seen_commits.insert(*c));

            let new_mutations: Vec<_> = mut_store
                .get_entries(&commits, &commits)
                .unwrap()
                .into_iter()
                .map(|e| CommitMutationsResponse {
                    mutation: local_mutation_to_edenapi(e),
                })
                .collect();
            if new_mutations.is_empty() {
                break;
            }

            for m in new_mutations.iter() {
                commits.push(m.mutation.successor);
                commits.extend_from_slice(&m.mutation.predecessors);
                commits.extend_from_slice(&m.mutation.split);
            }

            mutations.extend(new_mutations);
        }

        Ok(mutations)
    }

    async fn process_files_upload(
        &self,
        data: Vec<(AnyFileContentId, Bytes)>,
        bubble_id: Option<NonZeroU64>,
        copy_from_bubble_id: Option<NonZeroU64>,
        _lookup_policy: UploadLookupPolicy,
    ) -> Result<Response<UploadToken>, SaplingRemoteApiError> {
        debug!(?data, "process_files_upload");

        self.refresh_for_api();

        if bubble_id.is_some() || copy_from_bubble_id.is_some() {
            return Err(self.not_implemented_error(
                "EagerRepo does not support bubble_id".to_string(),
                "process_files_upload",
            ));
        }

        let mut res = Vec::with_capacity(data.len());
        for (id, data) in data {
            match self.add_sha1_blob_for_api(
                self.sha1_from_anyid(AnyId::AnyFileContentId(id), "process_files_upload")?,
                data,
                "process_files_upload",
            ) {
                Err(err) => res.push(Err(err)),
                Ok(()) => res.push(Ok(UploadToken {
                    data: UploadTokenData {
                        id: AnyId::AnyFileContentId(id),
                        bubble_id: None,
                        metadata: None,
                    },
                    signature: Default::default(),
                })),
            }
        }

        self.flush_for_api("process_files_upload").await?;

        Ok(convert_to_response(res))
    }

    async fn upload_filenodes_batch(
        &self,
        items: Vec<HgFilenodeData>,
    ) -> Result<Response<UploadTokensResponse>, SaplingRemoteApiError> {
        debug!(?items, "upload_filenodes_batch");

        self.refresh_for_api();

        let mut res = Vec::with_capacity(items.len());
        for data in items {
            let content_sha1 = self.sha1_from_anyid(
                data.file_content_upload_token.data.id,
                "upload_filesnodes_batch",
            )?;
            // NOTE: "raw_text" is pure content without hg/git SHA1 frames!
            let raw_text = self.get_sha1_blob_for_api(content_sha1, "upload_filenodes_batch")?;

            let mut content_with_parents =
                Vec::<u8>::with_capacity(raw_text.len() + 40 + 4 + data.metadata.len());
            let (mut p1, mut p2) = data.parents.into_nodes();
            if p2 < p1 {
                std::mem::swap(&mut p1, &mut p2);
            }
            content_with_parents.extend_from_slice(p1.as_ref());
            content_with_parents.extend_from_slice(p2.as_ref());

            // see sapling.filelog.filelog.add
            if raw_text.starts_with(b"\x01\n") || !data.metadata.is_empty() {
                content_with_parents.extend_from_slice(b"\x01\n");
                content_with_parents.extend(data.metadata);
                content_with_parents.extend_from_slice(b"\x01\n");
            }
            content_with_parents.extend_from_slice(raw_text.as_ref());

            self.add_sha1_blob_for_api(
                data.node_id,
                content_with_parents.into(),
                "upload_filenodes_batch",
            )?;

            res.push(Ok(UploadTokensResponse {
                token: UploadToken {
                    data: UploadTokenData {
                        id: AnyId::HgFilenodeId(data.node_id),
                        bubble_id: None,
                        metadata: None,
                    },
                    signature: Default::default(),
                },
            }));
        }

        self.flush_for_api("upload_filenodes_batch").await?;

        Ok(convert_to_response(res))
    }

    async fn upload_trees_batch(
        &self,
        items: Vec<UploadTreeEntry>,
    ) -> Result<Response<UploadTreeResponse>, SaplingRemoteApiError> {
        debug!(?items, "upload_trees_batch");

        self.refresh_for_api();

        let mut res = Vec::with_capacity(items.len());
        for tree in items {
            let mut content_with_parents = Vec::<u8>::with_capacity(tree.data.len() + 40);
            let (mut p1, mut p2) = tree.parents.into_nodes();
            if p2 < p1 {
                std::mem::swap(&mut p1, &mut p2);
            }
            content_with_parents.extend_from_slice(p1.as_ref());
            content_with_parents.extend_from_slice(p2.as_ref());
            content_with_parents.extend(tree.data);

            self.add_sha1_blob_for_api(
                tree.node_id,
                content_with_parents.into(),
                "upload_trees_batch",
            )?;

            res.push(Ok(UploadTreeResponse {
                token: UploadToken {
                    data: UploadTokenData {
                        id: AnyId::HgTreeId(tree.node_id),
                        bubble_id: None,
                        metadata: None,
                    },
                    signature: Default::default(),
                },
            }));
        }

        self.flush_for_api("upload_trees_batch").await?;

        Ok(convert_to_response(res))
    }

    async fn upload_changesets(
        &self,
        changesets: Vec<UploadHgChangeset>,
        mutations: Vec<HgMutationEntryContent>,
    ) -> Result<Response<UploadTokensResponse>, SaplingRemoteApiError> {
        debug!(?changesets, ?mutations, "upload_changesets");
        self.refresh_for_api();

        ::fail::fail_point!("eagerepo::api::uploadchangesets", |mode| {
            match mode.as_deref() {
                Some("error") => Err(SaplingRemoteApiError::HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "failpoint".to_string(),
                    headers: Default::default(),
                    url: self.url("upload_changesets"),
                }),
                Some("empty") => Ok(convert_to_response(Vec::new())),
                _ => panic!("invalid failpoint return mode - specify 'error' or 'empty'"),
            }
        });

        let mut res = Vec::with_capacity(changesets.len());
        for UploadHgChangeset {
            node_id: node,
            changeset_content: cs,
        } in changesets
        {
            let mut parents = Vec::with_capacity(2);
            let (p1, p2) = cs.parents.into_nodes();
            if !p1.is_null() {
                parents.push(p1);
                if !p2.is_null() {
                    parents.push(p2);
                }
            }

            // Eagerly compute augmented manifest data.
            if let Err(err) = self.derive_augmented_tree_recursively(cs.manifestid) {
                error!(?err, "error pre-deriving augmented tree data");
            }

            let text = match changeset_to_text(cs) {
                Ok(text) => text,
                Err(err) => {
                    res.push(Err(err.context("creating changeset text").into()));
                    continue;
                }
            };

            match self.add_commit(&parents, &text).await {
                Ok(actual_id) => {
                    if actual_id != node {
                        res.push(Err(anyhow!("commit id mismatch").into()));
                    } else {
                        res.push(Ok(UploadTokensResponse {
                            token: UploadToken {
                                data: UploadTokenData {
                                    id: AnyId::HgChangesetId(node),
                                    bubble_id: None,
                                    metadata: None,
                                },
                                signature: Default::default(),
                            },
                        }));
                    }
                }
                Err(err) => {
                    // edenapi_upload.py has the expectation that errors are not
                    // propagated by the server. "failed" commits are simply not returned.
                    // I don't think that is good, but let's go with the flow for now.
                    error!(?err, "error inserting commit");
                    continue;
                }
            }
        }

        {
            let mut mut_store = self.mut_store.lock().await;
            for m in mutations {
                if let Err(err) = mut_store.add(&edenapi_mutation_to_local(m)) {
                    return Err(SaplingRemoteApiError::HttpError {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        message: format!("error inserting mutation entry: {:?}", err),
                        headers: Default::default(),
                        url: self.url("upload_changesets"),
                    });
                }
            }
        }

        self.flush_for_api("upload_changesets").await?;

        Ok(convert_to_response(res))
    }

    async fn lookup_batch(
        &self,
        items: Vec<AnyId>,
        bubble_id: Option<NonZeroU64>,
        copy_from_bubble_id: Option<NonZeroU64>,
    ) -> Result<Vec<LookupResponse>, SaplingRemoteApiError> {
        debug!(?items, "lookup_batch");

        self.refresh_for_api();

        if bubble_id.is_some() || copy_from_bubble_id.is_some() {
            return Err(self.not_implemented_error(
                "EagerRepo does not support bubble_id".to_string(),
                "lookup_batch",
            ));
        }

        let mut res = Vec::with_capacity(items.len());
        for item in items {
            let sha1 = self.sha1_from_anyid(item, "lookup_batch")?;

            match self.get_sha1_blob(sha1) {
                Ok(None) => {
                    res.push(LookupResponse {
                        result: LookupResult::NotPresent(IndexableId {
                            id: item,
                            bubble_id: None,
                        }),
                    });
                }
                Ok(Some(_)) => {
                    res.push(LookupResponse {
                        result: LookupResult::Present(UploadToken {
                            data: UploadTokenData {
                                id: item,
                                bubble_id: None,
                                metadata: None,
                            },
                            signature: Default::default(),
                        }),
                    });
                }
                Err(e) => {
                    return Err(SaplingRemoteApiError::HttpError {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        message: format!("{:?}", e),
                        headers: Default::default(),
                        url: self.url("lookup_batch"),
                    });
                }
            }
        }

        Ok(res)
    }

    async fn commit_translate_id(
        &self,
        commits: Vec<CommitId>,
        scheme: CommitIdScheme,
        from_repo: Option<String>,
        to_repo: Option<String>,
        _lookup_behavior: Option<String>,
    ) -> Result<Response<CommitTranslateIdResponse>, SaplingRemoteApiError> {
        debug!("files {commits:?} -> {scheme:?}");

        if std::env::var_os("TESTTMP").is_none() {
            return Err(SaplingRemoteApiError::NotSupported);
        }

        // Implement a dummy "Bonsai" translation for testing.

        if from_repo.is_some() || to_repo.is_some() {
            return Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::BAD_REQUEST,
                message: "from_repo and to_repo not supported".to_string(),
                headers: Default::default(),
                url: self.url("commit_translate_id"),
            });
        }

        if !matches!(scheme, CommitIdScheme::Hg | CommitIdScheme::Bonsai) {
            return Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::BAD_REQUEST,
                message: "only hg and bonsai supported".to_string(),
                headers: Default::default(),
                url: self.url("commit_translate_id"),
            });
        }

        let mut res = Vec::new();
        for commit in commits {
            let translated = match &commit {
                CommitId::Hg(hg) => {
                    if scheme == CommitIdScheme::Hg {
                        commit.clone()
                    } else {
                        let mut fake_bonsai = [0u8; 32];
                        fake_bonsai[..20].copy_from_slice(hg.as_ref());
                        CommitId::Bonsai(fake_bonsai.into())
                    }
                }
                CommitId::Bonsai(bz) => {
                    if scheme == CommitIdScheme::Hg {
                        let mut hg = [0u8; 20];
                        hg[..].copy_from_slice(&bz.as_ref()[..20]);
                        CommitId::Hg(hg.into())
                    } else {
                        commit.clone()
                    }
                }
                _ => {
                    return Err(SaplingRemoteApiError::HttpError {
                        status: StatusCode::BAD_REQUEST,
                        message: "only hg and bonsai supported".to_string(),
                        headers: Default::default(),
                        url: self.url("commit_translate_id"),
                    });
                }
            };

            res.push(Ok(CommitTranslateIdResponse { commit, translated }));
        }

        Ok(convert_to_response(res))
    }

    async fn suffix_query(
        &self,
        commit: CommitId,
        suffixes: Vec<String>,
        prefix: Option<Vec<String>>,
    ) -> Result<Response<SuffixQueryResponse>, SaplingRemoteApiError> {
        debug!("suffix_query");
        // TODO(T189729875) Make this react to committed files
        //let files = self.files();
        let _ = (commit, prefix);
        let mut res = vec![];
        for suffix in suffixes {
            match suffix.clone().as_str() {
                ".cpp" => {
                    let from_string = RepoPathBuf::from_string("ranier.cpp".to_string());
                    let file_path = from_string.unwrap();
                    res.push(Ok(SuffixQueryResponse { file_path }));
                    let from_string = RepoPathBuf::from_string("fuji/peak.cpp".to_string());
                    let file_path = from_string.unwrap();
                    res.push(Ok(SuffixQueryResponse { file_path }));
                }
                ".txt" => {
                    let from_string = RepoPathBuf::from_string("foo.txt".to_string());
                    let file_path = from_string.unwrap();
                    res.push(Ok(SuffixQueryResponse { file_path }));
                    let from_string = RepoPathBuf::from_string("baz.txt".to_string());
                    let file_path = from_string.unwrap();
                    res.push(Ok(SuffixQueryResponse { file_path }));
                }
                ".rs" => {
                    let from_string = RepoPathBuf::from_string("bar.rs".to_string());
                    let file_path = from_string.unwrap();
                    res.push(Ok(SuffixQueryResponse { file_path }));
                }
                _ => {}
            }
        }
        Ok(convert_to_response(res))
    }

    async fn land_stack(
        &self,
        bookmark: String,
        head: HgId,
        base: HgId,
        pushvars: HashMap<String, String>,
    ) -> Result<LandStackResponse, SaplingRemoteApiError> {
        let _ = pushvars;
        let latest_bookmark_id = match self.get_bookmark(&bookmark) {
            Ok(Some(id)) => id,
            _ => {
                return Err(SaplingRemoteApiError::HttpError {
                    status: StatusCode::NOT_FOUND,
                    message: format!("bookmark {} was not found", bookmark),
                    headers: Default::default(),
                    url: self.url("land_stack"),
                });
            }
        };

        let roots = vec![base];
        let heads = vec![head];

        let source_commits = self
            .dag()
            .await
            .only(to_set(&heads), to_set(&roots))
            .await
            .map_err(map_dag_err)?;
        let commits_stream = source_commits.iter_rev().await.map_err(map_dag_err)?;
        let commits_stream: BoxStream<edenapi::Result<HgId>> = commits_stream
            .then(|v| async move {
                let v = v?;
                let hgid = HgId::from_slice(v.as_ref()).unwrap();
                Ok(hgid)
            })
            .map_err(map_dag_err)
            .boxed();
        let commits: Vec<HgId> = commits_stream.try_collect().await?;

        if latest_bookmark_id == base {
            // no changes on server side, just move the bookmark
            EagerRepo::set_bookmark(self, &bookmark, Some(head)).unwrap();
            let mut old_to_new_hgids = HashMap::new();
            for commit in commits {
                old_to_new_hgids.insert(
                    HgId::from_slice(commit.as_ref()).unwrap(),
                    HgId::from_slice(commit.as_ref()).unwrap(),
                );
            }
            let data = LandStackData {
                new_head: head,
                old_to_new_hgids,
            };
            self.flush_for_api("land_stack").await?;
            return Ok(LandStackResponse { data: Ok(data) });
        } else {
            let head_manifest = self.commit_to_manifest(head).await.map_err(map_crate_err)?;
            let base_manifest = self.commit_to_manifest(base).await.map_err(map_crate_err)?;
            let bookmark_manifest = self
                .commit_to_manifest(latest_bookmark_id)
                .await
                .map_err(map_crate_err)?;

            let conflicts =
                pushrebase_conflicts(&base_manifest, &bookmark_manifest, &head_manifest)?;
            if !conflicts.is_empty() {
                let e =
                    ServerError::generic(format!("Conflicts while pushrebasing: {:?}", conflicts));

                return Ok(LandStackResponse { data: Err(e) });
            }

            let mut old_to_new_hgids = HashMap::new();
            let mut base_commit = base;
            let mut dest_commit = latest_bookmark_id;
            for commit in commits {
                let new_commit = pushrebase_one(self, base_commit, commit, dest_commit).await?;
                old_to_new_hgids.insert(commit, new_commit);

                base_commit = commit;
                dest_commit = new_commit;
            }

            let new_head = old_to_new_hgids[&head];
            EagerRepo::set_bookmark(self, &bookmark, Some(new_head)).map_err(map_crate_err)?;
            let data = LandStackData {
                new_head,
                old_to_new_hgids,
            };
            self.flush_for_api("land_stack").await?;
            return Ok(LandStackResponse { data: Ok(data) });
        }

        async fn pushrebase_one(
            repo: &EagerRepo,
            base_commit: HgId,
            source_commit: HgId,
            dest_commit: HgId,
        ) -> anyhow::Result<HgId> {
            let base_manifest = repo.commit_to_manifest(base_commit).await?;
            let source_manifest = repo.commit_to_manifest(source_commit).await?;
            let dest_manifest = repo.commit_to_manifest(dest_commit).await?;

            let mut new_manifest = dest_manifest.clone();
            let matcher = AlwaysMatcher::new();

            // generate new manifest
            for e in base_manifest.diff(&source_manifest, matcher)? {
                let e = e?;
                match e.diff_type {
                    DiffType::LeftOnly(_) => {
                        new_manifest.remove(&e.path)?;
                    }
                    DiffType::Changed(_, right) => {
                        new_manifest.insert(e.path, right)?;
                    }
                    DiffType::RightOnly(right) => {
                        new_manifest.insert(e.path.clone(), right)?;
                    }
                }
            }

            let new_tree_id = match repo.store.format {
                SerializationFormat::Hg => {
                    let new_parents = vec![&dest_manifest];
                    let mut manifest_id: Option<HgId> = None;
                    for (path, hgid, raw, p1, p2) in new_manifest.finalize(new_parents)? {
                        let insert_opts = InsertOpts {
                            parents: vec![p1, p2],
                            kind: Kind::Tree,
                            ..Default::default()
                        };
                        repo.store.insert_data(insert_opts, &path, &raw)?;
                        if path.is_empty() {
                            manifest_id = Some(hgid);
                        }
                    }
                    match manifest_id {
                        Some(manifest_id) => manifest_id,
                        None => {
                            return Err(anyhow!(
                                "empty commit is not supported: {}",
                                source_commit.to_hex()
                            ));
                        }
                    }
                }
                SerializationFormat::Git => new_manifest.flush()?,
            };

            // generate new commit
            let old_raw_text = match repo.store.get_content(source_commit)? {
                None => {
                    return Err(anyhow!(
                        "commit content cannot be found: {}",
                        source_commit.to_hex()
                    ));
                }
                Some(raw_text) => raw_text,
            };
            let mut new_raw_text: Vec<u8> = Vec::new();
            write!(new_raw_text, "{}", new_tree_id)?;
            new_raw_text.extend_from_slice(&old_raw_text[HgId::hex_len()..]);

            let commit_parents = vec![dest_commit];
            let new_commit = repo.add_commit(&commit_parents, &new_raw_text).await?;

            Ok(new_commit)
        }

        /// `left` and `right` are considerered to be conflict free, if none of the element
        /// from `left` is prefix of element from `right`, and vice versa.
        fn pushrebase_conflicts(
            mbase: &TreeManifest,
            mleft: &TreeManifest,
            mright: &TreeManifest,
        ) -> anyhow::Result<Vec<(RepoPathBuf, RepoPathBuf)>> {
            let matcher = AlwaysMatcher::new();
            let mut left = mbase
                .diff(mleft, matcher.clone())?
                .map(|e| e.map(|e| e.path))
                .collect::<anyhow::Result<Vec<_>>>()?;
            left.sort_unstable();
            let mut left_iter = left.into_iter();

            let mut right = mbase
                .diff(mright, matcher)?
                .map(|e| e.map(|e| e.path))
                .collect::<anyhow::Result<Vec<_>>>()?;
            right.sort_unstable();
            let mut right_iter = right.into_iter();

            let mut conflicts = Vec::new();
            let mut state = (left_iter.next(), right_iter.next());
            let is_case_sensitive = true;
            loop {
                state = match state {
                    (Some(l), Some(r)) => match l.cmp(&r) {
                        Ordering::Equal => {
                            conflicts.push((l.clone(), r.clone()));
                            (left_iter.next(), right_iter.next())
                        }
                        Ordering::Less => {
                            if r.starts_with(&l, is_case_sensitive) {
                                conflicts.push((l.clone(), r.clone()));
                            }
                            (left_iter.next(), Some(r))
                        }
                        Ordering::Greater => {
                            if l.starts_with(&r, is_case_sensitive) {
                                conflicts.push((l.clone(), r.clone()));
                            }
                            (Some(l), right_iter.next())
                        }
                    },
                    _ => break,
                }
            }

            Ok(conflicts)
        }
    }
}

fn sha1_blob_to_parents_body(
    data: &Bytes,
    format: SerializationFormat,
) -> anyhow::Result<(Parents, Bytes)> {
    let (parents, body) = match format {
        SerializationFormat::Hg => {
            let (body, p2, p1) = hg_sha1_deserialize(data)?;
            (Parents::new(p1, p2), data.slice_to_bytes(body))
        }
        SerializationFormat::Git => {
            let body = git_sha1_deserialize(data)?.0;
            (Parents::default(), data.slice_to_bytes(body))
        }
    };
    Ok((parents, body))
}

fn file_body_to_file_content_and_copy_from(
    body: &Bytes,
    format: SerializationFormat,
) -> (Bytes, Bytes) {
    match format {
        SerializationFormat::Hg => format_util::split_hg_file_metadata(body),
        SerializationFormat::Git => (body.clone(), Bytes::new()),
    }
}

fn edenapi_mutation_to_local(m: HgMutationEntryContent) -> MutationEntry {
    MutationEntry {
        succ: m.successor,
        preds: m.predecessors,
        split: m.split,
        op: m.op,
        user: String::from_utf8_lossy(&m.user).to_string(),
        time: m.time,
        tz: m.tz,
        extra: m
            .extras
            .into_iter()
            .map(|e| (e.key.into_boxed_slice(), e.value.into_boxed_slice()))
            .collect(),
    }
}

fn local_mutation_to_edenapi(m: MutationEntry) -> HgMutationEntryContent {
    HgMutationEntryContent {
        successor: m.succ,
        predecessors: m.preds,
        split: m.split,
        op: m.op,
        user: m.user.into_bytes(),
        time: m.time,
        tz: m.tz,
        extras: m
            .extra
            .into_iter()
            .map(|(k, v)| Extra {
                key: k.to_vec(),
                value: v.to_vec(),
            })
            .collect(),
    }
}

fn changeset_to_text(mut cs: HgChangesetContent) -> anyhow::Result<Vec<u8>> {
    // TODO: validate stuff better
    let mut text = Vec::<u8>::new();

    writeln!(text, "{}", cs.manifestid)?;

    writeln!(text, "{}", String::from_utf8(cs.user)?)?;

    write!(text, "{} {}", cs.time, cs.tz)?;

    if !cs.extras.is_empty() {
        write!(text, " ")?;

        let mut extras: Vec<(String, String)> = Vec::with_capacity(cs.extras.len());
        for extra in cs.extras {
            extras.push((
                String::from_utf8(extra.key)?,
                String::from_utf8(extra.value)?,
            ));
        }
        extras.sort_by(|a, b| a.0.cmp(&b.0));
        for (idx, (k, v)) in extras.into_iter().enumerate() {
            let extra = format!("{k}:{v}")
                .replace('\\', r"\\")
                .replace('\n', r"\n")
                .replace('\r', r"\r")
                .replace('\0', r"\0");
            if idx > 0 {
                text.push(0);
            }
            write!(text, "{extra}")?;
        }
    }

    text.push(b'\n');

    cs.files.sort();
    for file in cs.files {
        writeln!(text, "{file}")?;
    }

    text.push(b'\n');

    text.extend_from_slice(&cs.message);

    Ok(text)
}

impl EagerRepo {
    fn get_sha1_blob_for_api(&self, id: HgId, handler: &str) -> edenapi::Result<minibytes::Bytes> {
        // Emulate the HTTP errors.
        match self.opt_sha1_blob_for_api(id, handler)? {
            None => Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::NOT_FOUND,
                message: format!("{} cannot be found", id.to_hex()),
                headers: Default::default(),
                url: self.url(handler),
            }),
            Some(data) => Ok(data),
        }
    }

    fn opt_sha1_blob_for_api(
        &self,
        id: HgId,
        handler: &str,
    ) -> edenapi::Result<Option<minibytes::Bytes>> {
        // Emulate the HTTP errors.
        match self.get_sha1_blob(id) {
            Ok(None) => {
                trace!(" not found: {}", id.to_hex());
                Ok(None)
            }
            Ok(Some(data)) => {
                trace!(" found: {}, {} bytes", id.to_hex(), data.len());
                Ok(Some(data))
            }
            Err(e) => Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("{:?}", e),
                headers: Default::default(),
                url: self.url(handler),
            }),
        }
    }

    fn add_sha1_blob_for_api(
        &self,
        id: HgId,
        blob: minibytes::Bytes,
        handler: &str,
    ) -> edenapi::Result<()> {
        let actual_id = match self.add_sha1_blob(blob.as_ref()) {
            Ok(actual_id) => actual_id,
            Err(e) => {
                return Err(SaplingRemoteApiError::HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!("{:?}", e),
                    headers: Default::default(),
                    url: self.url(handler),
                });
            }
        };
        if id != actual_id {
            return Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::BAD_REQUEST,
                message: "content hash mismatch".to_string(),
                headers: Default::default(),
                url: self.url(handler),
            });
        }

        Ok(())
    }

    fn opt_augmented_tree_blob_with_digest_for_api(
        &self,
        id: HgId,
        handler: &str,
    ) -> edenapi::Result<Option<minibytes::Bytes>> {
        // Emulate the HTTP errors.
        match self.derive_augmented_tree_recursively(id) {
            Ok(None) => {
                trace!(" not found: {}", id.to_hex());
                Ok(None)
            }
            Ok(Some(data)) => {
                trace!(" found: {}, {} bytes", id.to_hex(), data.len());
                Ok(Some(data))
            }
            Err(e) => Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("{:?}", e),
                headers: Default::default(),
                url: self.url(handler),
            }),
        }
    }

    async fn get_augmented_tree_blob_with_digest_for_api(
        &self,
        id: HgId,
        handler: &str,
    ) -> edenapi::Result<minibytes::Bytes> {
        // Emulate the HTTP errors.
        match self.opt_augmented_tree_blob_with_digest_for_api(id, handler)? {
            None => Err(SaplingRemoteApiError::HttpError {
                status: StatusCode::NOT_FOUND,
                message: format!("{} cannot be found", id.to_hex()),
                headers: Default::default(),
                url: self.url(handler),
            }),
            Some(data) => Ok(data),
        }
    }

    async fn flush_for_api(&self, handler: &str) -> edenapi::Result<()> {
        self.flush()
            .await
            .map_err(|err| SaplingRemoteApiError::HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("error flushing dag/store: {:?}", err),
                headers: Default::default(),
                url: self.url(handler),
            })
    }

    /// Not implement error.
    fn not_implemented_error(&self, message: String, handler: &str) -> SaplingRemoteApiError {
        SaplingRemoteApiError::HttpError {
            status: StatusCode::NOT_IMPLEMENTED,
            message,
            headers: Default::default(),
            url: self.url(handler),
        }
    }

    /// Provide the URL for HTTP error reporting.
    fn url(&self, handler: &str) -> String {
        format!("eager://{}/{}", self.dir.display(), handler)
    }

    fn sha1_from_anyid(&self, id: AnyId, handler: &str) -> edenapi::Result<HgId> {
        match id {
            AnyId::HgFilenodeId(hgid) => Ok(hgid),
            AnyId::HgTreeId(hgid) => Ok(hgid),
            AnyId::HgChangesetId(hgid) => Ok(hgid),
            AnyId::AnyFileContentId(AnyFileContentId::Sha1(id)) => {
                Ok(HgId::from_byte_array(id.into_byte_array()))
            }
            _ => Err(self.not_implemented_error(
                format!("id type {:?} not supported by EagerRepo", id),
                handler,
            )),
        }
    }
}

/// Optionally build `SaplingRemoteApi` from config.
///
/// If the config does not specify eagerepo-based `SaplingRemoteApi`, return `Ok(None)`.
pub fn edenapi_from_config(
    config: &dyn Config,
) -> edenapi::Result<Option<Arc<dyn SaplingRemoteApi>>> {
    for (section, name) in [("paths", "default"), ("edenapi", "url")] {
        if let Ok(url) = config.must_get::<RepoUrl>(section, name) {
            trace!(
                target: "eagerepo::edenapi_from_config",
                "attempt to create EagerRepo as SaplingRemoteApi from config {section}.{name}={url}",
            );
            if let Some(path) = EagerRepo::url_to_dir(&url) {
                let repo = EagerRepo::open(&path)
                    .map_err(|e| edenapi::SaplingRemoteApiError::Other(e.into()))?;
                return Ok(Some(Arc::new(repo)));
            }
        }
    }
    Ok(None)
}

fn default_response_meta() -> ResponseMeta {
    ResponseMeta {
        version: Version::HTTP_11,
        status: StatusCode::OK,
        server: Some("EagerRepo".to_string()),
        ..Default::default()
    }
}

/// Extract rename metadata from filelog header (if rename exists).
/// data is not prefixed by hashes.
///
/// See `filelog.py:parsemeta`.
fn extract_rename(data: &[u8]) -> Option<Key> {
    if data.starts_with(b"\x01\n") {
        let data = &data[2..];
        if let Some(pos) = data.windows(2).position(|needle| needle == b"\x01\n") {
            let header = String::from_utf8_lossy(&data[..pos]);
            let mut path = None;
            let mut rev = None;
            for line in header.lines() {
                let kv: Vec<&str> = line.split(": ").collect();
                if let [k, v] = &kv[..] {
                    if *k == "copy" {
                        path = RepoPathBuf::from_string(v.to_string()).ok();
                    } else if *k == "copyrev" {
                        rev = HgId::from_hex(v.as_bytes()).ok();
                    }
                }
            }
            if let (Some(path), Some(rev)) = (path, rev) {
                return Some(Key { path, hgid: rev });
            }
        }
    }
    None
}

/// Convert `Vec<T>` to `Response<T>`.
fn convert_to_response<T: Send + Sync + 'static>(values: Vec<edenapi::Result<T>>) -> Response<T> {
    Response {
        stats: Box::pin(async { Ok(Default::default()) }),
        entries: Box::pin(futures::stream::iter(values)),
    }
}

fn check_convert_to_hgid<'a>(vertexes: impl Iterator<Item = &'a Vertex>) -> edenapi::Result<()> {
    for v in vertexes {
        let _ = HgId::from_slice(v.as_ref()).map_err(|e| SaplingRemoteApiError::Other(e.into()))?;
    }
    Ok(())
}

fn to_vec_vertex(ids: &[HgId]) -> Vec<Vertex> {
    ids.iter().map(|i| Vertex::copy_from(i.as_ref())).collect()
}

fn to_set(ids: &[HgId]) -> Set {
    let vertexes = to_vec_vertex(ids);
    Set::from_static_names(vertexes)
}

fn map_dag_err(e: dag::Error) -> SaplingRemoteApiError {
    SaplingRemoteApiError::Other(e.into())
}

fn map_crate_err(e: crate::Error) -> SaplingRemoteApiError {
    SaplingRemoteApiError::Other(e.into())
}

fn debug_key_list(keys: &[Key]) -> String {
    debug_list(keys, |k| k.hgid.to_hex())
}

fn debug_spec_list(reqs: &[FileSpec]) -> String {
    debug_list(reqs, |s| format!("{s:?}"))
}

fn debug_hgid_list(ids: &[HgId]) -> String {
    debug_list(ids, |i| i.to_hex())
}

fn debug_string_list(s: &[String]) -> String {
    debug_list(s, |s| s.clone())
}

fn debug_list<T>(keys: &[T], func: impl Fn(&T) -> String) -> String {
    let limit = 5;
    let msg = keys
        .iter()
        .take(limit)
        .map(func)
        .collect::<Vec<_>>()
        .join(", ");
    if keys.len() > limit {
        format!("{} and {} more", msg, keys.len() - limit)
    } else {
        msg
    }
}
