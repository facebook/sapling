/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use context::PerfCounterType;
use edenapi_types::AnyId;
use edenapi_types::Batch;
use edenapi_types::CheckManifestPermissionRequest;
use edenapi_types::CheckManifestPermissionResponse;
use edenapi_types::CheckPathPermissionAclEntry;
use edenapi_types::CheckPathPermissionData;
use edenapi_types::CheckPathPermissionRequest;
use edenapi_types::CheckPathPermissionResponse;
use edenapi_types::FileAuxData;
use edenapi_types::SaplingRemoteApiServerError;
use edenapi_types::SaplingRemoteApiServerErrorKind;
use edenapi_types::ServerError;
use edenapi_types::TreeAttributes;
use edenapi_types::TreeAuxData;
use edenapi_types::TreeChildEntry;
use edenapi_types::TreeEntry;
use edenapi_types::TreeRequest;
use edenapi_types::UploadToken;
use edenapi_types::UploadTreeRequest;
use edenapi_types::UploadTreeResponse;
use edenapi_types::wire::WireTreeRequest;
use futures::Future;
use futures::FutureExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use gotham_ext::error::HttpError;
use gotham_ext::handler::SlapiCommitIdentityScheme;
use gotham_ext::middleware::request_context::RequestContext;
use gotham_ext::middleware::scuba::ScubaMiddlewareState;
use gotham_ext::response::TryIntoResponse;
use manifest::Entry;
use manifest::Manifest;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api::errors::MononokeError;
use mononoke_api_hg::HgAugmentedTreeRestrictionContext;
use mononoke_api_hg::HgDataContext;
use mononoke_api_hg::HgDataId;
use mononoke_api_hg::HgRepoContext;
use mononoke_api_hg::HgTreeContext;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use permission_checker::MononokeIdentitySetExt;
use rate_limiting::Metric;
use rate_limiting::Scope;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use serde::Deserialize;
use stats::define_stats;
use stats::prelude::TimeseriesStatic;
use types::Key;
use types::RepoPathBuf;

use super::HandlerInfo;
use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use super::handler::SaplingRemoteApiContext;
use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::handlers::git_objects::fetch_git_object;
use crate::middleware::request_dumper::RequestDumper;
use crate::utils::custom_cbor_stream;
use crate::utils::get_repo;
use crate::utils::parse_wire_request;
use crate::utils::to_hg_path_nonroot;

define_stats! {
    prefix = "mononoke.trees";
    manifests_served: timeseries(Rate, Sum),
    trees_batch_keys_requested: timeseries(Rate, Sum),
}

// The size is optimized for the batching settings in EdenFs.
const MAX_CONCURRENT_TREE_FETCHES_PER_REQUEST: usize = 128;
const MAX_CONCURRENT_METADATA_FETCHES_PER_TREE_FETCH: usize = 100;
const MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST: usize = 100;
const LARGE_TREE_METADATA_LIMIT: usize = 25000;

const ROUTE_ORIGINAL_TO_AUGMENTED_HG_MANIFEST: &str =
    "scm/mononoke:route_original_to_augmented_hg_manifest";

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct TreeParams {
    repo: String,
}

/// Fetch the tree nodes requested by the client.
pub async fn trees(state: &mut State) -> Result<impl TryIntoResponse + use<>, HttpError> {
    let params = TreeParams::take_from(state);

    state.put(HandlerInfo::new(
        &params.repo,
        SaplingRemoteApiMethod::Trees,
    ));

    let rctx = RequestContext::borrow_from(state).clone();
    let sctx = ServerContext::borrow_from(state);
    let slapi_flavour = SlapiCommitIdentityScheme::borrow_from(state).clone();
    let repo: HgRepoContext<Repo> =
        get_repo(sctx, &rctx, &params.repo, Metric::TotalManifests).await?;
    let request = parse_wire_request::<WireTreeRequest>(state).await?;
    if let Some(rd) = RequestDumper::try_borrow_mut_from(state) {
        rd.add_request(&request);
    };

    ScubaMiddlewareState::try_set_sampling_rate(state, nonzero_ext::nonzero!(256_u64));

    Ok(custom_cbor_stream(
        super::monitor_request(state, fetch_all_trees(repo, request, slapi_flavour)),
        |tree_entry| tree_entry.as_ref().err(),
    ))
}

/// Fetch trees for all of the requested keys concurrently.
fn fetch_all_trees<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    request: TreeRequest,
    flavour: SlapiCommitIdentityScheme,
) -> impl Stream<Item = Result<TreeEntry, SaplingRemoteApiServerError>> {
    let ctx = repo.ctx().clone();

    STATS::trees_batch_keys_requested.add_value(request.keys.len() as i64);

    let fetches = request.keys.into_iter().map(move |key| match flavour {
        SlapiCommitIdentityScheme::Git => fetch_git_object_as_tree(key.clone(), repo.clone())
            .map(|r| r.map_err(|e| SaplingRemoteApiServerError::with_key(key, e)))
            .left_future(),
        SlapiCommitIdentityScheme::Hg => fetch_tree(repo.clone(), key.clone(), request.attributes)
            .map(|r| r.map_err(|e| tree_fetch_error_to_slapi_error(key, e)))
            .right_future(),
    });

    stream::iter(fetches)
        .buffer_unordered(MAX_CONCURRENT_TREE_FETCHES_PER_REQUEST)
        .inspect_ok(move |_| {
            ctx.session()
                .bump_load(Metric::TotalManifests, Scope::Regional, 1.0);
            STATS::manifests_served.add_value(1);
        })
}

fn tree_fetch_error_to_slapi_error(key: Key, err: Error) -> SaplingRemoteApiServerError {
    let permission_request_group =
        err.chain()
            .find_map(|cause| match cause.downcast_ref::<MononokeError>() {
                Some(MononokeError::RestrictedPathsAuthorizationError(err))
                    if err.is_manifest_access() =>
                {
                    Some(err.permission_request_group().to_string())
                }
                _ => None,
            });

    if let Some(permission_request_group) = permission_request_group {
        SaplingRemoteApiServerError {
            err: SaplingRemoteApiServerErrorKind::PermissionDenied {
                tree_id: key.hgid,
                request_acl: permission_request_group,
            },
            key: Some(key),
        }
    } else {
        SaplingRemoteApiServerError::with_key(key, err)
    }
}

// Sapling wants to use trees the same way for Hg and Git, so shaping somehow
// the git object to fit within the defined TreeEntry structure.
async fn fetch_git_object_as_tree<R: MononokeRepo>(
    key: Key,
    repo: HgRepoContext<R>,
) -> Result<TreeEntry, Error> {
    let git_object = fetch_git_object(key.hgid, &repo).await;

    Ok(TreeEntry {
        key: key.clone(),
        data: git_object.ok().map(|o| o.bytes.into()),
        parents: None,
        children: None,
        tree_aux_data: None,
        // Path ACLs are not supported in Git
        has_acl: None,
    })
}

/// Fetch requested tree for a single key.
/// Note that this function consumes the repo context in order
/// to construct a tree context for the requested blob.
async fn fetch_tree<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    key: Key,
    attributes: TreeAttributes,
) -> Result<TreeEntry, Error> {
    let mut entry = TreeEntry::new(key.clone());
    let route_to_augmented = justknobs::eval(
        ROUTE_ORIGINAL_TO_AUGMENTED_HG_MANIFEST,
        repo.ctx()
            .metadata()
            .client_request_info()
            .map(|c| c.correlator.as_str()),
        Some(repo.repo().repo_identity().name()),
    );

    // The augmented manifest's content is byte-identical to the original Hg
    // manifest blob, so serving it for an original request still hash-verifies
    // on the client.
    if route_to_augmented || attributes.augmented_trees {
        let id = HgAugmentedManifestId::new(HgNodeHash::from(key.hgid));
        let perf_counter = if attributes.augmented_trees {
            PerfCounterType::EdenapiAugmentedTrees
        } else {
            PerfCounterType::EdenapiOriginalRoutedToAugmented
        };
        repo.ctx().perf_counters().increment_counter(perf_counter);

        let maybe_ctx = id
            .context(repo.clone())
            .await
            .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?;

        if let Some(ctx) = maybe_ctx {
            let populate_all_metadata = route_to_augmented;

            entry.with_tree_aux_data(TreeAuxData {
                augmented_manifest_id: ctx.augmented_manifest_id().clone().into(),
                augmented_manifest_size: ctx.augmented_manifest_size(),
            });
            entry.with_has_acl(ctx.is_restricted().await?);

            if attributes.parents || populate_all_metadata {
                entry.with_parents(Some(ctx.hg_parents().into()));
            }

            if attributes.child_metadata || populate_all_metadata {
                repo.ctx()
                    .perf_counters()
                    .increment_counter(PerfCounterType::EdenapiTreesAuxData);

                let child_restrictions = ctx
                    .children_restrictions(MAX_CONCURRENT_METADATA_FETCHES_PER_TREE_FETCH)
                    .await?;
                let children = ctx
                    .augmented_children_entries()
                    .map(|(path, augmented_entry)| {
                        fetch_augmented_child_metadata(
                            &key,
                            path,
                            augmented_entry,
                            child_restrictions.get(path).copied().unwrap_or(false),
                        )
                    })
                    .collect();

                entry.with_children(Some(children));
            }

            if attributes.manifest_blob {
                let (data, _) = ctx
                    .content()
                    .await
                    .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?;

                entry.with_data(Some(data.into()));
            }

            return Ok(entry);
        } else if route_to_augmented {
            // Fail closed rather than falling back to the unprotected original
            // manifest. The augmented manifest cannot be derived on demand here
            // (derivation is per-changeset and needs the ACL overlay, but the
            // trees endpoint only has a manifest id), so a missing augmented
            // manifest must be backfilled out of band.
            repo.ctx()
                .perf_counters()
                .increment_counter(PerfCounterType::EdenapiAugmentedManifestMissUnderRoute);
            let repo_name = repo.repo().repo_identity().name();
            // Log the specific manifest id that needs backfilling so a miss is
            // actionable from scuba (the aggregate counter has no per-key id).
            repo.ctx()
                .scuba()
                .clone()
                .add("repo", repo_name)
                .add("hg_manifest_id", key.hgid.to_string())
                .log_with_msg(
                    "Augmented Hg manifest unavailable (route_original_to_augmented_hg_manifest)",
                    None,
                );
            return Err(MononokeError::NotAvailable(format!(
                "augmented Hg manifest unavailable for tented repo {repo_name}; \
                 the original (non-augmented) manifest is disabled"
            ))
            .into());
        } else {
            // If we don't have an augmented tree, fallback to the old way of fetching trees
            // Log the fallback to scuba
            repo.ctx()
                .perf_counters()
                .increment_counter(PerfCounterType::EdenapiAugmentedTreesFallback);
        }
    }

    let id = <HgManifestId as HgDataId<R>>::from_node_hash(HgNodeHash::from(key.hgid));

    let ctx = id
        .context(repo.clone())
        .await
        .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?
        .with_context(|| ErrorKind::KeyDoesNotExist(key.clone()))?;

    if attributes.manifest_blob {
        repo.ctx()
            .perf_counters()
            .increment_counter(PerfCounterType::EdenapiTrees);

        let (data, _) = ctx
            .content()
            .await
            .with_context(|| ErrorKind::TreeFetchFailed(key.clone()))?;

        entry.with_data(Some(data.into()));
    }

    if attributes.parents {
        entry.with_parents(Some(ctx.hg_parents().into()));
    }

    if attributes.child_metadata {
        repo.ctx()
            .perf_counters()
            .increment_counter(PerfCounterType::EdenapiTreesAuxData);

        if let Some(entries) = fetch_child_file_metadata_entries(&repo, &ctx).await? {
            let children: Vec<Result<TreeChildEntry, SaplingRemoteApiServerError>> = entries
                .buffer_unordered(MAX_CONCURRENT_METADATA_FETCHES_PER_TREE_FETCH)
                .map(|r| r.map_err(|e| SaplingRemoteApiServerError::with_key(key.clone(), e)))
                .collect()
                .await;

            entry.with_children(Some(children));
        }
    }

    Ok(entry)
}

/// Builds child metadata from preloaded augmented manifest entries.
fn fetch_augmented_child_metadata(
    key: &Key,
    path: &MPathElement,
    augmented_entry: &HgAugmentedManifestEntry,
    directory_has_acl: bool,
) -> Result<TreeChildEntry, SaplingRemoteApiServerError> {
    match augmented_entry {
        HgAugmentedManifestEntry::FileNode(file) => Ok(TreeChildEntry::new_file_entry(
            Key {
                hgid: file.filenode.into(),
                path: RepoPathBuf::from_string(path.to_string())
                    .map_err(|e| SaplingRemoteApiServerError::with_key(key.clone(), e))?,
            },
            FileAuxData {
                blake3: file.content_blake3.clone().into(),
                sha1: file.content_sha1.clone().into(),
                total_size: file.total_size.clone(),
                file_header_metadata: Some(
                    file.file_header_metadata
                        .clone()
                        .unwrap_or(Bytes::new())
                        .into(),
                ),
            }
            .into(),
        )),
        HgAugmentedManifestEntry::DirectoryNode(tree) => Ok(TreeChildEntry::new_directory_entry(
            Key {
                hgid: tree.treenode.into(),
                path: RepoPathBuf::from_string(path.to_string())
                    .map_err(|e| SaplingRemoteApiServerError::with_key(key.clone(), e))?,
            },
            TreeAuxData {
                augmented_manifest_id: tree.augmented_manifest_id.clone().into(),
                augmented_manifest_size: tree.augmented_manifest_size.clone(),
            },
            Some(directory_has_acl),
        )),
    }
}

async fn fetch_child_file_metadata_entries<'a, R: MononokeRepo>(
    repo: &'a HgRepoContext<R>,
    ctx: &'a HgTreeContext<R>,
) -> Result<
    Option<impl Stream<Item = impl Future<Output = Result<TreeChildEntry, Error>> + 'a> + 'a>,
    Error,
> {
    let manifest = ctx.clone().into_blob_manifest()?;
    if manifest.content().files.len() > LARGE_TREE_METADATA_LIMIT {
        return Ok(None);
    }
    let file_entries = manifest
        .list(repo.ctx(), repo.repo().repo_blobstore())
        .await?
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .filter_map(|(name, entry)| {
            if let Entry::Leaf((_, child_id)) = entry {
                Some((name, child_id))
            } else {
                None
            }
        });

    Ok(Some(
        stream::iter(file_entries)
            // .entries iterator is not `Send`
            .map({
                move |(name, child_id)| async move {
                    let name = RepoPathBuf::from_string(name.to_string())?;
                    let child_key = Key::new(name, child_id.into_nodehash().into());
                    fetch_child_file_metadata(repo, child_key.clone()).await
                }
            }),
    ))
}

async fn fetch_child_file_metadata<R: MononokeRepo>(
    repo: &HgRepoContext<R>,
    child_key: Key,
) -> Result<TreeChildEntry, Error> {
    let ctx = repo
        .file(HgFileNodeId::new(child_key.hgid.into()))
        .await?
        .ok_or_else(|| ErrorKind::FileFetchFailed(child_key.clone()))?;

    let metadata = ctx.content_metadata().await?;
    Ok(TreeChildEntry::new_file_entry(
        child_key,
        FileAuxData {
            total_size: metadata.total_size,
            sha1: metadata.sha1.into(),
            blake3: metadata.seeded_blake3.into(),
            file_header_metadata: Some(ctx.file_header_metadata().into()),
        }
        .into(),
    ))
}

/// Store the content of a single tree
async fn store_tree<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    item: UploadTreeRequest,
) -> Result<UploadTreeResponse, Error> {
    let upload_node_id = HgNodeHash::from(item.entry.node_id);
    let contents = item.entry.data;
    let p1 = item.entry.parents.p1().cloned().map(HgNodeHash::from);
    let p2 = item.entry.parents.p2().cloned().map(HgNodeHash::from);
    let computed_node_id = item.entry.computed_node_id.map(HgNodeHash::from);
    repo.store_tree(
        upload_node_id,
        p1,
        p2,
        Bytes::from(contents),
        computed_node_id,
    )
    .await?;
    Ok(UploadTreeResponse {
        token: UploadToken::new_fake_token(AnyId::HgTreeId(item.entry.node_id), None),
    })
}

/// Upload list of trees requested by the client (batch request).
pub struct UploadTreesHandler;

#[async_trait]
impl SaplingRemoteApiHandler for UploadTreesHandler {
    type Request = Batch<UploadTreeRequest>;
    type Response = UploadTreeResponse;

    const HTTP_METHOD: http::Method = http::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::UploadTrees;
    const ENDPOINT: &'static str = "/upload/trees";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let tokens = request
            .batch
            .into_iter()
            .map(move |item| store_tree(repo.clone(), item));

        Ok(stream::iter(tokens)
            .buffer_unordered(MAX_CONCURRENT_UPLOAD_TREES_PER_REQUEST)
            .boxed())
    }
}

pub struct CheckManifestPermissionHandler;

#[async_trait]
impl SaplingRemoteApiHandler for CheckManifestPermissionHandler {
    type Request = CheckManifestPermissionRequest;
    type Response = CheckManifestPermissionResponse;

    const HTTP_METHOD: http::Method = http::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CheckManifestPermission;
    const ENDPOINT: &'static str = "/check_manifest_permission";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();

        Ok(stream::iter(request.manifest_ids)
            .map(move |manifest_id| {
                let repo = repo.clone();
                async move {
                    let hg_manifest_id = HgManifestId::new(
                        HgNodeHash::from_bytes(manifest_id.as_ref())
                            .map_err(|e| anyhow::anyhow!("Invalid manifest id: {e}"))?,
                    );

                    let restriction_ctx = HgAugmentedTreeRestrictionContext::new_check_exists(
                        repo.clone(),
                        hg_manifest_id.into(),
                    )
                    .await?;
                    let restriction_checks = restriction_ctx.restriction_check().await?;
                    let has_access = restriction_checks
                        .iter()
                        .all(|check| check.has_authorization());

                    // TODO(T248658346): change the Eden API response to return
                    // all permission request groups instead of only the first one.
                    let permission_request_group = restriction_checks
                        .iter()
                        .find(|check| !check.has_authorization())
                        .map(|check| {
                            check
                                .restriction_info()
                                .permission_request_group
                                .to_string()
                        });

                    for check in &restriction_checks {
                        repo.ctx()
                            .scuba()
                            .clone()
                            .add("repo", repo.repo().repo_identity().name())
                            .add("edenapi_method", "check_manifest_permission")
                            .add_opt(
                                "edenapi_user",
                                repo.ctx()
                                    .metadata()
                                    .identities()
                                    .username()
                                    .map(ToString::to_string),
                            )
                            .add(
                                "unix_username",
                                repo.ctx()
                                    .metadata()
                                    .identities()
                                    .username()
                                    .map(ToString::to_string),
                            )
                            .add(
                                "restricted_path_acl",
                                check.restriction_info().repo_region_acl.clone(),
                            )
                            .add("has_restricted_path_acl_access", check.has_acl_access())
                            .add("is_allowlisted_tooling", check.is_allowlisted_tooling())
                            .add("is_rollout_allowlisted", check.is_rollout_allowlisted())
                            .add("has_restricted_path_access", check.has_authorization())
                            .add("is_admin_bypass", check.is_admin_bypass())
                            .log_with_msg("Checked manifest permission", None);
                    }

                    Ok(CheckManifestPermissionResponse {
                        manifest_id,
                        has_access,
                        request_acl: permission_request_group,
                    })
                }
            })
            .buffer_unordered(20)
            .boxed())
    }
}

pub struct CheckPathPermissionHandler;

#[async_trait]
impl SaplingRemoteApiHandler for CheckPathPermissionHandler {
    type Request = CheckPathPermissionRequest;
    type Response = CheckPathPermissionResponse;

    const HTTP_METHOD: http::Method = http::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CheckPathPermission;
    const ENDPOINT: &'static str = "/check_path_permission";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let repo_ctx = repo.repo_ctx().clone();
        let cs = repo_ctx
            .changeset(request.hg_cs_id)
            .await
            .context("Failed to resolve changeset")?
            .ok_or_else(|| anyhow::anyhow!("Changeset not found: {}", request.hg_cs_id))?;

        Ok(stream::iter(request.paths)
            .map(move |path| {
                let cs = cs.clone();
                async move {
                    let result = async {
                        let mpath = MPath::new(path.as_str().as_bytes())
                            .with_context(|| format!("Invalid path: {path}"))?;
                        let restriction_ctx = cs
                            .path_restriction(mpath)
                            .await
                            .with_context(|| format!("Failed to check path restriction: {path}"))?;
                        let restriction_infos = restriction_ctx.restriction_info(true).await?;

                        let restriction_entries = restriction_infos
                            .iter()
                            .map(|info| {
                                let restriction_root = to_hg_path_nonroot(info.restriction_root())
                                    .with_context(|| {
                                        format!(
                                            "Invalid restriction root: {}",
                                            info.restriction_root()
                                        )
                                    })?;
                                Ok(CheckPathPermissionAclEntry {
                                    restriction_root,
                                    repo_region_acl: info.repo_region_acl().to_string(),
                                    permission_request_group: info
                                        .permission_request_group()
                                        .to_string(),
                                })
                            })
                            .collect::<anyhow::Result<Vec<_>>>()?;

                        let has_access = restriction_infos
                            .iter()
                            .all(|info| info.has_access.unwrap_or(false));

                        Ok::<_, anyhow::Error>(CheckPathPermissionData {
                            has_access,
                            restriction_entries,
                        })
                    }
                    .await
                    .map_err(|err| ServerError::generic(format!("{err:#}")));

                    Ok(CheckPathPermissionResponse::from_result(path, result))
                }
            })
            .buffer_unordered(20)
            .boxed())
    }

    fn extract_in_band_error(response: &Self::Response) -> Option<anyhow::Error> {
        response
            .result
            .as_ref()
            .err()
            .map(|err| anyhow::format_err!("{err:?}"))
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use mononoke_macros::mononoke;
    use restricted_paths::PermissionRequestGroup;
    use restricted_paths::RestrictedManifestId;
    use restricted_paths::RestrictedPathAccess;
    use restricted_paths::RestrictedPathsAuthorizationError;
    use types::HgId;

    use super::*;

    #[mononoke::test]
    fn test_tree_fetch_error_to_slapi_error_preserves_manifest_permission_denied() -> Result<()> {
        let key = test_key()?;
        let err = restricted_paths_error(
            RestrictedPathAccess::Manifest(RestrictedManifestId::from(
                "1111111111111111111111111111111111111111",
            )),
            "REPO_REGION:test_acl",
        )?;

        let slapi_error = tree_fetch_error_to_slapi_error(key.clone(), err);
        match slapi_error.err {
            SaplingRemoteApiServerErrorKind::PermissionDenied {
                tree_id,
                request_acl: permission_request_group,
            } => {
                assert_eq!(tree_id, key.hgid);
                assert_eq!(permission_request_group, "REPO_REGION:test_acl");
            }
            err => anyhow::bail!("expected PermissionDenied, got {err:?}"),
        }
        assert_eq!(slapi_error.key, Some(key));
        Ok(())
    }

    #[mononoke::test]
    fn test_tree_fetch_error_to_slapi_error_ignores_path_permission_denied() -> Result<()> {
        let key = test_key()?;
        let err = restricted_paths_error(
            RestrictedPathAccess::Path(MPath::new("restricted")?),
            "REPO_REGION:test_acl",
        )?;

        let slapi_error = tree_fetch_error_to_slapi_error(key.clone(), err);
        if let SaplingRemoteApiServerErrorKind::PermissionDenied { .. } = slapi_error.err {
            anyhow::bail!("path access denial should not be converted to tree PermissionDenied");
        }
        assert_eq!(slapi_error.key, Some(key));
        Ok(())
    }

    fn restricted_paths_error(
        access: RestrictedPathAccess,
        permission_request_group: &str,
    ) -> Result<Error> {
        let permission_request_group: PermissionRequestGroup = permission_request_group.parse()?;
        Ok(Error::new(MononokeError::RestrictedPathsAuthorizationError(
            RestrictedPathsAuthorizationError::new(access, permission_request_group),
        ))
        .context("failed to fetch tree"))
    }

    fn test_key() -> Result<Key> {
        Ok(Key {
            hgid: HgId::null_id().clone(),
            path: RepoPathBuf::from_string("restricted".to_string())?,
        })
    }
}
