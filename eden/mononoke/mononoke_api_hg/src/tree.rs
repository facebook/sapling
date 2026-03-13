/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgBlobEnvelope;
use mercurial_types::HgManifestEnvelope;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgParents;
use mercurial_types::HgPreloadedAugmentedManifest;
use mercurial_types::fetch_augmented_manifest_envelope_opt;
use mercurial_types::fetch_manifest_envelope;
use mercurial_types::fetch_manifest_envelope_opt;
use mononoke_api::MononokeRepo;
use mononoke_api::PathAccessInfo;
use mononoke_api::errors::MononokeError;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::hash::Blake3;
use repo_blobstore::RepoBlobstoreRef;
use restricted_paths::ArcRestrictedPaths;
use restricted_paths::ManifestId;
use restricted_paths::ManifestType;
use restricted_paths::RestrictedPathsArc;
use restricted_paths::has_read_access_to_repo_region_acls;
use revisionstore_types::Metadata;

use super::HgDataContext;
use super::HgDataId;
use super::HgRepoContext;

#[derive(Clone)]
pub struct HgTreeContext<R> {
    #[allow(dead_code)]
    repo_ctx: HgRepoContext<R>,
    envelope: HgManifestEnvelope,
}

impl<R: MononokeRepo> HgTreeContext<R> {
    /// Create a new `HgTreeContext`, representing a single tree manifest node.
    ///
    /// The tree node must exist in the repository. To construct an `HgTreeContext`
    /// for a tree node that may not exist, use `new_check_exists`.
    pub async fn new(
        repo_ctx: HgRepoContext<R>,
        manifest_id: HgManifestId,
    ) -> Result<Self, MononokeError> {
        let ctx = repo_ctx.ctx();
        let blobstore = repo_ctx.repo().repo_blobstore();
        let envelope = fetch_manifest_envelope(ctx, blobstore, manifest_id).await?;
        Ok(Self { repo_ctx, envelope })
    }

    pub async fn new_check_exists(
        repo_ctx: HgRepoContext<R>,
        manifest_id: HgManifestId,
    ) -> Result<Option<Self>, MononokeError> {
        let ctx = repo_ctx.ctx();
        let blobstore = repo_ctx.repo().repo_blobstore();
        let envelope = fetch_manifest_envelope_opt(ctx, blobstore, manifest_id).await?;

        let manifest_id = ManifestId::new(manifest_id.as_bytes().into());
        restricted_paths::spawn_enforce_restricted_manifest_access(
            ctx,
            repo_ctx.repo_ctx().repo().restricted_paths_arc().clone(),
            manifest_id,
            ManifestType::Hg,
            "hg_tree_context_new_check_exists",
        )
        .await?;

        Ok(envelope.map(move |envelope| Self { repo_ctx, envelope }))
    }

    /// Get the content for this tree manifest node in the format expected
    /// by Mercurial's data storage layer.
    pub fn content_bytes(&self) -> Bytes {
        self.envelope.contents().clone()
    }

    pub fn into_blob_manifest(self) -> anyhow::Result<mercurial_types::blobs::HgBlobManifest> {
        mercurial_types::blobs::HgBlobManifest::parse(self.envelope)
    }
}

#[derive(Clone)]
pub struct HgAugmentedTreeContext<R> {
    #[allow(dead_code)]
    repo_ctx: HgRepoContext<R>,
    preloaded_manifest: HgPreloadedAugmentedManifest,
}

impl<R: MononokeRepo> HgAugmentedTreeContext<R> {
    /// Create a new `HgAugmentedTreeContext`, representing a single augmented tree manifest node.
    pub async fn new_check_exists(
        repo_ctx: HgRepoContext<R>,
        augmented_manifest_id: HgAugmentedManifestId,
    ) -> Result<Option<Self>, MononokeError> {
        let ctx = repo_ctx.ctx();
        let blobstore = repo_ctx.repo().repo_blobstore();
        let envelope =
            fetch_augmented_manifest_envelope_opt(ctx, blobstore, augmented_manifest_id).await?;

        let manifest_id = ManifestId::new(augmented_manifest_id.as_bytes().into());
        restricted_paths::spawn_enforce_restricted_manifest_access(
            ctx,
            repo_ctx.repo_ctx().repo().restricted_paths_arc().clone(),
            manifest_id,
            ManifestType::HgAugmented,
            "hg_augmented_tree_context_new_check_exists",
        )
        .await?;

        if let Some(envelope) = envelope {
            let preloaded_manifest =
                HgPreloadedAugmentedManifest::load_from_sharded(envelope, ctx, blobstore).await?;
            Ok(Some(Self {
                repo_ctx,
                preloaded_manifest,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn augmented_manifest_id(&self) -> Blake3 {
        self.preloaded_manifest.augmented_manifest_id
    }

    pub fn augmented_manifest_size(&self) -> u64 {
        self.preloaded_manifest.augmented_manifest_size
    }

    pub fn augmented_children_entries(
        &self,
    ) -> impl Iterator<Item = &(MPathElement, HgAugmentedManifestEntry)> {
        self.preloaded_manifest.children_augmented_metadata.iter()
    }

    /// Get the content for this tree manifest node in the format expected
    /// by Mercurial's data storage layer.
    pub fn content_bytes(&self) -> Bytes {
        self.preloaded_manifest.contents.clone()
    }
}

#[async_trait]
impl<R: MononokeRepo> HgDataContext<R> for HgTreeContext<R> {
    type NodeId = HgManifestId;

    /// Get the manifest node hash (HgManifestId) for this tree.
    ///
    /// This should be same as the HgManifestId specified when this context was created,
    /// but the value returned here comes from the data loaded from Mononoke.
    fn node_id(&self) -> HgManifestId {
        HgManifestId::new(self.envelope.node_id())
    }

    /// Get the parents of this tree node in a strongly-typed manner.
    ///
    /// Useful for implementing anything that needs to traverse the history
    /// of tree nodes, or otherwise needs to use make further queries using
    /// the returned `HgManifestId`s.
    fn parents(&self) -> (Option<HgManifestId>, Option<HgManifestId>) {
        let (p1, p2) = self.envelope.parents();
        (p1.map(HgManifestId::new), p2.map(HgManifestId::new))
    }

    /// Get the parents of this tree node in a format that can be easily
    /// sent to the Mercurial client as part of a serialized response.
    fn hg_parents(&self) -> HgParents {
        self.envelope.get_parents()
    }

    /// The manifest envelope actually contains the underlying tree bytes
    /// inline, so they can be accessed synchronously and infallibly with the
    /// `content_bytes` method. This method just wraps the bytes in a TryFuture
    /// that immediately succeeds. Note that tree blobs don't have associated
    /// metadata so we just return the default value here.
    async fn content(&self) -> Result<(Bytes, Metadata), MononokeError> {
        Ok((self.content_bytes(), Metadata::default()))
    }
}

#[async_trait]
impl<R: MononokeRepo> HgDataContext<R> for HgAugmentedTreeContext<R> {
    type NodeId = HgManifestId;

    /// Get the manifest node hash (HgAugmentedManifestId) for this tree.
    ///
    /// This should be same as the HgAugmentedManifestId specified when this context was created,
    /// but the value returned here comes from the data loaded from Mononoke.
    fn node_id(&self) -> HgManifestId {
        HgManifestId::new(self.preloaded_manifest.hg_node_id)
    }

    /// Get the parents of this tree node in a strongly-typed manner.
    ///
    /// Useful for implementing anything that needs to traverse the history
    /// of tree nodes, or otherwise needs to use make further queries using
    /// the returned `HgManifestId`s.
    fn parents(&self) -> (Option<HgManifestId>, Option<HgManifestId>) {
        (
            self.preloaded_manifest.p1.map(HgManifestId::new),
            self.preloaded_manifest.p2.map(HgManifestId::new),
        )
    }

    /// Get the parents of this tree node in a format that can be easily
    /// sent to the Mercurial client as part of a serialized response.
    fn hg_parents(&self) -> HgParents {
        HgParents::new(self.preloaded_manifest.p1, self.preloaded_manifest.p2)
    }

    async fn content(&self) -> Result<(Bytes, Metadata), MononokeError> {
        Ok((self.content_bytes(), Metadata::default()))
    }
}

#[async_trait]
impl<R: MononokeRepo> HgDataId<R> for HgManifestId {
    type Context = HgTreeContext<R>;

    fn from_node_hash(hash: HgNodeHash) -> Self {
        HgManifestId::new(hash)
    }

    async fn context(
        self,
        repo: HgRepoContext<R>,
    ) -> Result<Option<HgTreeContext<R>>, MononokeError> {
        HgTreeContext::new_check_exists(repo, self).await
    }
}

#[async_trait]
impl<R: MononokeRepo> HgDataId<R> for HgAugmentedManifestId {
    type Context = HgAugmentedTreeContext<R>;

    fn from_node_hash(hash: HgNodeHash) -> Self {
        HgAugmentedManifestId::new(hash)
    }

    async fn context(
        self,
        repo: HgRepoContext<R>,
    ) -> Result<Option<HgAugmentedTreeContext<R>>, MononokeError> {
        HgAugmentedTreeContext::new_check_exists(repo, self).await
    }
}

/// Context for querying restriction metadata about an Hg manifest node.
///
/// Like `ChangesetPathRestrictionContext` in mononoke_api, this type does NOT
/// enforce Path ACL access checks in its constructor. This avoids the circular
/// dependency where callers would need permission to instantiate the context
/// that checks permissions.
///
/// This type never returns manifest content — only restriction metadata
/// (ACLs, access checks).
pub struct HgAugmentedTreeRestrictionContext<R> {
    repo_ctx: HgRepoContext<R>,
    manifest_id: HgAugmentedManifestId,
}

impl<R: MononokeRepo> HgAugmentedTreeRestrictionContext<R> {
    /// Create a new restriction context for the given manifest ID.
    ///
    /// Checks repo read access but does NOT enforce Path ACL checks.
    pub async fn new(
        repo_ctx: HgRepoContext<R>,
        manifest_id: HgAugmentedManifestId,
    ) -> Result<Self, MononokeError> {
        repo_ctx
            .repo_ctx()
            .authorization_context()
            .require_full_repo_read(repo_ctx.ctx(), repo_ctx.repo_ctx().repo())
            .await?;

        Ok(Self {
            repo_ctx,
            manifest_id,
        })
    }

    /// Query restriction info for this manifest node.
    ///
    /// Returns the restriction info for this specific manifest, or `None` if
    /// the manifest is not at a restricted path.
    ///
    /// # NOTE: Temporary implementation
    /// Uses the ManifestIdStore to map manifest IDs to paths, then checks the
    /// path-based restriction config for the most specific matching root.
    ///
    /// # Long-term implementation
    /// Will use AclManifests — the HgAugmentedManifest's `acl_manifest_directory_id`
    /// pointer to look up ACL info directly, without needing the ManifestIdStore
    /// path resolution step.
    ///
    /// Unlike `ChangesetPathRestrictionContext::restriction_info`,
    /// which can traverse the AclManifest from the root path and aggregate all
    /// path restrictions, this primitive can only access the AclManifest from
    /// the given manifest ids. It doesn't have visibility into its parents,
    /// so it will only return PathAccessInfo if the manifest belongs to
    /// a restriction root.
    ///
    /// This is acceptable **under an important assumption**: in order to fetch
    /// any child manifest, the client must already have access to the parent
    /// manifest, which means they have permission to access the directory.
    // TODO(T248660146): update to use AclManifest instead of ManifestIdStore.
    pub async fn restriction_info(&self) -> Result<Option<PathAccessInfo>, MononokeError> {
        let is_enabled = justknobs::eval(
            "scm/mononoke:enable_server_side_path_acls",
            None,
            Some("HgAugmentedTreeRestrictionContext::restriction_info"),
        )?;

        if !is_enabled {
            return Err(MononokeError::NotAvailable(
                "HgAugmentedTreeRestrictionContext::restriction_info is not enabled".to_string(),
            ));
        }

        let restricted_paths = self.repo_ctx.repo().restricted_paths_arc();

        if !restricted_paths.has_restricted_paths() {
            return Ok(None);
        }

        let manifest_id_bytes = ManifestId::new(self.manifest_id.as_bytes().into());

        // Look up which paths this manifest ID maps to
        let paths = restricted_paths
            .config_based()
            .manifest_id_store()
            .get_paths_by_manifest_id(self.repo_ctx.ctx(), &manifest_id_bytes, &ManifestType::Hg)
            .await
            .map_err(MononokeError::from)?;

        if paths.is_empty() {
            return Ok(None);
        }

        // A manifest ID can map to multiple paths (identical content at different
        // locations). Check each path and return the first restriction match found.
        stream::iter(paths)
            .then(|path| {
                let restricted_paths = restricted_paths.clone();
                async move { self.check_path_restriction(restricted_paths, &path).await }
            })
            .try_filter_map(futures::future::ok)
            .boxed()
            .try_next()
            .await
    }

    /// Check the restriction for a single path, returning the most specific
    /// matching restriction root's info. A path can be covered by multiple
    /// nested roots (e.g. `foo/` and `foo/bar/`); we return the deepest one,
    /// matching how AclManifests will work (each directory has one ACL).
    async fn check_path_restriction(
        &self,
        restricted_paths: ArcRestrictedPaths,
        path: &NonRootMPath,
    ) -> Result<Option<PathAccessInfo>, MononokeError> {
        // Find the most specific (deepest) restriction root covering this path.
        let most_specific = restricted_paths
            .config()
            .path_acls
            .iter()
            .filter(|(root, _)| root.is_prefix_of(path) || *root == path)
            .max_by_key(|(root, _)| root.num_components());

        let (restriction_root, acl) = match most_specific {
            Some((root, acl)) => (root.clone(), acl.clone()),
            None => return Ok(None),
        };

        let repo_region_acl = acl.to_string();

        let has_access = has_read_access_to_repo_region_acls(
            self.repo_ctx.ctx(),
            restricted_paths.acl_provider(),
            &[&acl],
        )
        .await?;

        // TODO(T248658346): look up permission_request_group from .slacl file
        let request_acl = repo_region_acl.clone();

        Ok(Some(PathAccessInfo {
            restriction: restricted_paths::PathRestrictionInfo {
                restriction_root,
                repo_region_acl,
                request_acl,
            },
            has_access: Some(has_access),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;

    use blobstore::Loadable;
    use clientinfo::ClientEntryPoint;
    use clientinfo::ClientInfo;
    use clientinfo::ClientRequestInfo;
    use context::CoreContext;
    use context::SessionContainer;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use futures::TryStreamExt;
    use manifest::ManifestOps;
    use maplit::hashmap;
    use mercurial_derivation::DeriveHgChangeset;
    use mercurial_types::NULL_HASH;
    use metaconfig_types::RestrictedPathsConfig;
    use metadata::Metadata;
    use mononoke_api::repo::Repo;
    use mononoke_api::repo::RepoContext;
    use mononoke_api::specifiers::HgChangesetId;
    use mononoke_macros::mononoke;
    use mononoke_types::RepositoryId;
    use mononoke_types::path::MPath;
    use permission_checker::Acl;
    use permission_checker::Acls;
    use permission_checker::InternalAclProvider;
    use permission_checker::MononokeIdentity;
    use permission_checker::MononokeIdentitySet;
    use pretty_assertions::assert_eq;
    use repo_derived_data::RepoDerivedDataArc;
    use restricted_paths::RestrictedPaths;
    use restricted_paths::RestrictedPathsConfigBased;
    use restricted_paths::RestrictedPathsManifestIdCacheBuilder;
    use restricted_paths::SqlRestrictedPathsManifestIdStoreBuilder;
    use scuba_ext::MononokeScubaSampleBuilder;
    use sql_construct::SqlConstruct;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::RepoContextHgExt;

    #[mononoke::fbinit_test]
    async fn test_hg_tree_context(fb: FacebookInit) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::get_repo::<Repo>(fb).await);
        let rctx = RepoContext::new_test(ctx.clone(), repo.clone()).await?;

        // Get the HgManifestId of the root tree manifest for a commit in this repo.
        // (Commit hash was found by inspecting the source of the `fixtures` crate.)
        let hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let hg_cs = hg_cs_id.load(&ctx, rctx.repo().repo_blobstore()).await?;
        let manifest_id = hg_cs.manifestid();

        let hg = rctx.hg();

        let tree = HgTreeContext::new(hg.clone(), manifest_id).await?;
        assert_eq!(manifest_id, tree.node_id());

        let content = tree.content_bytes();

        // The content here is the representation of the format in which
        // the Mercurial client would store a tree manifest node.
        let expected = &b"1\0b8e02f6433738021a065f94175c7cd23db5f05be\nfiles\0b8e02f6433738021a065f94175c7cd23db5f05be\n"[..];
        assert_eq!(content, expected);

        let tree = HgTreeContext::new_check_exists(hg.clone(), manifest_id).await?;
        assert!(tree.is_some());

        let null_id = HgManifestId::new(NULL_HASH);
        let null_tree = HgTreeContext::new(hg.clone(), null_id).await;
        assert!(null_tree.is_err());

        let null_tree = HgTreeContext::new_check_exists(hg.clone(), null_id).await?;
        assert!(null_tree.is_none());

        Ok(())
    }

    // ---- restriction context test helpers ----

    /// Create a CoreContext with a test user identity for ACL checking.
    async fn create_test_ctx(fb: FacebookInit) -> CoreContext {
        let client_identity = MononokeIdentity::new("USER", "myusername0");
        let mut cri = ClientRequestInfo::new(ClientEntryPoint::Tests);
        cri.set_main_id("user:myusername0".to_string());
        let client_info = ClientInfo::new_with_client_request_info(cri);

        let identities = BTreeSet::from([client_identity]);
        let metadata = {
            let mut md = Metadata::new(
                Some(&"restricted_paths_test".to_string()),
                identities,
                false,
                false,
                None,
                None,
            )
            .await;
            md.add_client_info(client_info);
            Arc::new(md)
        };
        let session_container = SessionContainer::builder(fb).metadata(metadata).build();
        CoreContext::test_mock_session(session_container)
    }

    /// Create an ACL config where myusername0 has access to myusername_project
    /// but NOT to restricted_acl.
    fn create_test_acls() -> anyhow::Result<Acls> {
        let default_user = MononokeIdentity::from_str("USER:myusername0")?;
        let default_users = {
            let mut users = MononokeIdentitySet::new();
            users.insert(default_user);
            users
        };

        Ok(Acls {
            repos: hashmap! {
                "default".to_string() => Arc::new(Acl {
                    actions: hashmap! {
                        "read".to_string() => default_users.clone(),
                        "write".to_string() => default_users,
                    },
                }),
            },
            repo_regions: hashmap! {
                "myusername_project".to_string() => Arc::new(Acl {
                    actions: hashmap! {
                        "read".to_string() => {
                            let mut users = MononokeIdentitySet::new();
                            users.insert(MononokeIdentity::from_str("USER:myusername0")?);
                            users
                        },
                    },
                }),
                "restricted_acl".to_string() => Arc::new(Acl {
                    actions: hashmap! {
                        "read".to_string() => {
                            let mut users = MononokeIdentitySet::new();
                            users.insert(MononokeIdentity::from_str("USER:another_user")?);
                            users
                        },
                    },
                }),
            },
            tiers: HashMap::new(),
            workspaces: HashMap::new(),
            groups: HashMap::new(),
        })
    }

    /// Build a test repo with restricted paths, real ACL checking, and ManifestIdStore.
    async fn setup_restricted_repo(
        ctx: &CoreContext,
        path_acls: Vec<(&str, &str)>,
    ) -> anyhow::Result<Repo> {
        let repo_id = RepositoryId::new(0);

        let path_acls_map: HashMap<NonRootMPath, MononokeIdentity> = path_acls
            .into_iter()
            .map(|(path, acl_str)| {
                (
                    NonRootMPath::new(path).expect("Failed to create NonRootMPath"),
                    MononokeIdentity::from_str(acl_str).expect("Failed to parse MononokeIdentity"),
                )
            })
            .collect();

        let manifest_id_store = Arc::new(
            SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()
                .expect("Failed to create Sqlite connection")
                .with_repo_id(repo_id),
        );

        let config = RestrictedPathsConfig {
            path_acls: path_acls_map,
            use_manifest_id_cache: true,
            cache_update_interval_ms: 5,
            soft_path_acls: Vec::new(),
            tooling_allowlist_group: None,
            conditional_enforcement_acls: Vec::new(),
            acl_file_name: RestrictedPathsConfig::default().acl_file_name,
        };

        let cache = Arc::new(
            RestrictedPathsManifestIdCacheBuilder::new(ctx.clone(), manifest_id_store.clone())
                .with_refresh_interval(std::time::Duration::from_millis(
                    config.cache_update_interval_ms,
                ))
                .build()
                .await?,
        );

        // Write ACLs to temp file for InternalAclProvider
        let acls = create_test_acls()?;
        let mut temp_file = tempfile::NamedTempFile::new()?;
        std::io::Write::write_all(
            &mut temp_file,
            serde_json::to_string_pretty(&acls)?.as_bytes(),
        )?;
        std::io::Write::flush(&mut temp_file)?;
        let acl_path = temp_file.into_temp_path().keep()?;
        let acl_provider = InternalAclProvider::from_file(&acl_path)?;

        let scuba = MononokeScubaSampleBuilder::with_discard();

        // Build a repo first to get ArcRepoDerivedData
        let repo: Repo = TestRepoFactory::new(ctx.fb)?.build().await?;
        let repo_derived_data = repo.repo_derived_data_arc();
        let config_based = Arc::new(RestrictedPathsConfigBased::new(
            config,
            manifest_id_store,
            Some(cache),
        ));

        let restricted_paths = Arc::new(RestrictedPaths::new(
            config_based,
            acl_provider,
            scuba,
            true, // use_acl_manifest
            repo_derived_data,
        )?);

        let repo: Repo = TestRepoFactory::new(ctx.fb)?
            .with_restricted_paths(restricted_paths)
            .build()
            .await?;

        Ok(repo)
    }

    // ---- restriction context tests ----

    #[mononoke::fbinit_test]
    async fn test_check_manifest_permission_no_restrictions_configured(
        fb: FacebookInit,
    ) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);
        let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
        let repo_ctx = RepoContext::new_test(ctx, Arc::new(repo)).await?;
        let hg = repo_ctx.hg();

        // Use a dummy manifest ID — no restrictions configured, so early return
        let dummy_manifest_id: HgAugmentedManifestId = HgManifestId::new(
            HgNodeHash::from_static_str("0000000000000000000000000000000000000000")?,
        )
        .into();

        let restriction_ctx = HgAugmentedTreeRestrictionContext::new(hg, dummy_manifest_id).await?;
        let result = restriction_ctx.restriction_info().await?;
        assert!(
            result.is_none(),
            "expected None when no restrictions configured"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_check_manifest_permission_manifest_not_in_store(
        fb: FacebookInit,
    ) -> anyhow::Result<()> {
        let ctx = create_test_ctx(fb).await;
        let repo =
            setup_restricted_repo(&ctx, vec![("restricted/dir", "REPO_REGION:restricted_acl")])
                .await?;

        // Create commit touching only unrestricted paths
        let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("public/file.txt", "content")
            .commit()
            .await?;

        // Derive Hg manifest (populates ManifestIdStore, but only for restricted paths)
        let hg_cs_id = repo.derive_hg_changeset(&ctx, bcs_id).await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let hg_cs = hg_cs_id.load(&ctx, repo.repo_blobstore()).await?;
        let root_mfid = hg_cs.manifestid();

        let repo_ctx = RepoContext::new_test(ctx, Arc::new(repo)).await?;
        let hg = repo_ctx.hg();

        // Root manifest is unrestricted, so not in the ManifestIdStore
        let restriction_ctx = HgAugmentedTreeRestrictionContext::new(hg, root_mfid.into()).await?;
        let result = restriction_ctx.restriction_info().await?;
        assert!(result.is_none(), "expected None for unrestricted manifest");

        Ok(())
    }

    /// Helper: create a restricted repo, commit a file, derive Hg manifest,
    /// find the manifest ID for `target_manifest_path`, and call restriction_info().
    async fn get_restriction_info_for_manifest(
        fb: FacebookInit,
        path_acls: Vec<(&str, &str)>,
        file_to_add: (&str, &str),
        target_manifest_path: &str,
    ) -> anyhow::Result<Option<PathAccessInfo>> {
        let ctx = create_test_ctx(fb).await;
        let repo = setup_restricted_repo(&ctx, path_acls).await?;

        let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(file_to_add.0, file_to_add.1)
            .commit()
            .await?;

        let hg_cs_id = repo.derive_hg_changeset(&ctx, bcs_id).await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let hg_cs = hg_cs_id.load(&ctx, repo.repo_blobstore()).await?;
        let root_mfid = hg_cs.manifestid();

        let blobstore = Arc::new(repo.repo_blobstore().clone());
        let target_path = MPath::try_from(target_manifest_path)?;
        let target_mfid = root_mfid
            .list_tree_entries(ctx.clone(), blobstore)
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .find(|(path, _)| *path == target_path)
            .map(|(_, mfid)| mfid)
            .ok_or_else(|| anyhow::anyhow!("manifest for {} not found", target_manifest_path))?;

        let repo_ctx = RepoContext::new_test(ctx, Arc::new(repo)).await?;
        let hg = repo_ctx.hg();

        let restriction_ctx =
            HgAugmentedTreeRestrictionContext::new(hg, target_mfid.into()).await?;
        restriction_ctx.restriction_info().await.map_err(Into::into)
    }

    #[mononoke::fbinit_test]
    async fn test_check_manifest_permission_restricted_path_with_access(
        fb: FacebookInit,
    ) -> anyhow::Result<()> {
        let result = get_restriction_info_for_manifest(
            fb,
            vec![("user_project/foo", "REPO_REGION:myusername_project")],
            ("user_project/foo/bar/a", "content"),
            "user_project/foo",
        )
        .await?;

        let info = result.expect("expected restriction info for restricted root manifest");
        assert_eq!(
            info.restriction_root(),
            &NonRootMPath::new("user_project/foo")?,
            "restriction root should match the configured root"
        );
        assert_eq!(
            info.has_access,
            Some(true),
            "user should have access to myusername_project"
        );
        assert_eq!(info.repo_region_acl(), "REPO_REGION:myusername_project");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_check_manifest_permission_restricted_path_without_access(
        fb: FacebookInit,
    ) -> anyhow::Result<()> {
        let result = get_restriction_info_for_manifest(
            fb,
            vec![("restricted/dir", "REPO_REGION:restricted_acl")],
            ("restricted/dir/a", "secret"),
            "restricted/dir",
        )
        .await?;

        let info = result.expect("expected restriction info for restricted root manifest");
        assert_eq!(
            info.restriction_root(),
            &NonRootMPath::new("restricted/dir")?,
            "restriction root should match the configured root"
        );
        assert_eq!(
            info.has_access,
            Some(false),
            "user should not have access to restricted_acl"
        );
        assert_eq!(info.repo_region_acl(), "REPO_REGION:restricted_acl");

        Ok(())
    }

    /// When nested restriction roots exist (e.g. `foo/` and `foo/bar/`),
    /// querying the manifest at the inner root should return the most specific
    /// root's info — matching how AclManifests will work (each directory has
    /// exactly one ACL).
    #[mononoke::fbinit_test]
    async fn test_check_manifest_permission_nested_roots(fb: FacebookInit) -> anyhow::Result<()> {
        // Query the manifest at foo/bar, which is covered by both roots.
        // restriction_info should return foo/bar's ACL (the most specific).
        let result = get_restriction_info_for_manifest(
            fb,
            vec![
                ("foo", "REPO_REGION:myusername_project"),
                ("foo/bar", "REPO_REGION:restricted_acl"),
            ],
            ("foo/bar/qux/file.txt", "content"),
            "foo/bar",
        )
        .await?;

        let info = result.expect("expected restriction info for nested restricted root");
        assert_eq!(
            info.restriction_root(),
            &NonRootMPath::new("foo/bar")?,
            "should return the most specific (deepest) restriction root"
        );
        assert_eq!(
            info.has_access,
            Some(false),
            "user should not have access to restricted_acl"
        );
        assert_eq!(info.repo_region_acl(), "REPO_REGION:restricted_acl");

        Ok(())
    }

    /// Edge case: When two directories with identical content are both
    /// restriction roots, they share a single Hg manifest ID.
    /// In the long-term solution, this will only happen if they have the same
    /// ACL file, which means they also share `PathAccessInfo`.
    #[mononoke::fbinit_test]
    async fn test_check_manifest_permission_same_manifest_multiple_paths(
        fb: FacebookInit,
    ) -> anyhow::Result<()> {
        let ctx = create_test_ctx(fb).await;
        let repo = setup_restricted_repo(
            &ctx,
            vec![
                ("dir_a", "REPO_REGION:restricted_acl"),
                ("dir_b", "REPO_REGION:restricted_acl"),
            ],
        )
        .await?;

        // Create two directories with identical content. Identical content
        // produces the same Hg manifest ID, so the ManifestIdStore will map
        // that single ID to both paths.
        let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("dir_a/file.txt", "same content")
            .add_file("dir_b/file.txt", "same content")
            .commit()
            .await?;

        let hg_cs_id = repo.derive_hg_changeset(&ctx, bcs_id).await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let hg_cs = hg_cs_id.load(&ctx, repo.repo_blobstore()).await?;
        let root_mfid = hg_cs.manifestid();

        let blobstore = Arc::new(repo.repo_blobstore().clone());
        let entries: Vec<_> = root_mfid
            .list_tree_entries(ctx.clone(), blobstore)
            .try_collect()
            .await?;

        let dir_a_mfid = entries
            .iter()
            .find(|(path, _)| *path == MPath::try_from("dir_a").expect("valid path"))
            .map(|(_, mfid)| *mfid)
            .ok_or_else(|| anyhow::anyhow!("manifest for dir_a not found"))?;

        let dir_b_mfid = entries
            .iter()
            .find(|(path, _)| *path == MPath::try_from("dir_b").expect("valid path"))
            .map(|(_, mfid)| *mfid)
            .ok_or_else(|| anyhow::anyhow!("manifest for dir_b not found"))?;

        // Verify they share the same manifest ID (identical content)
        assert_eq!(
            dir_a_mfid, dir_b_mfid,
            "identical directories should produce the same manifest ID"
        );

        let repo_ctx = RepoContext::new_test(ctx, Arc::new(repo)).await?;
        let hg = repo_ctx.hg();

        // Query the shared manifest ID. The store maps it to both dir_a and dir_b.
        // restriction_info should return info for one of them.
        let restriction_ctx = HgAugmentedTreeRestrictionContext::new(hg, dir_a_mfid.into()).await?;
        let result = restriction_ctx.restriction_info().await?;

        let info =
            result.expect("expected restriction info when manifest maps to restricted root paths");
        assert_eq!(
            info.has_access,
            Some(false),
            "user should not have access to restricted_acl"
        );
        assert_eq!(info.repo_region_acl(), "REPO_REGION:restricted_acl");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_check_manifest_permission_justknob_disabled(
        fb: FacebookInit,
    ) -> anyhow::Result<()> {
        use futures::FutureExt;
        use justknobs::test_helpers::JustKnobsInMemory;
        use justknobs::test_helpers::KnobVal;
        use justknobs::test_helpers::with_just_knobs_async;

        let ctx = create_test_ctx(fb).await;
        let repo =
            setup_restricted_repo(&ctx, vec![("restricted/dir", "REPO_REGION:restricted_acl")])
                .await?;

        let bcs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("restricted/dir/a", "content")
            .commit()
            .await?;

        let hg_cs_id = repo.derive_hg_changeset(&ctx, bcs_id).await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let hg_cs = hg_cs_id.load(&ctx, repo.repo_blobstore()).await?;
        let root_mfid = hg_cs.manifestid();

        let blobstore = Arc::new(repo.repo_blobstore().clone());
        let restricted_mfid = {
            let target_path = MPath::try_from("restricted/dir")?;
            let entries: Vec<_> = root_mfid
                .list_tree_entries(ctx.clone(), blobstore)
                .try_collect()
                .await?;
            entries
                .into_iter()
                .find(|(path, _)| *path == target_path)
                .map(|(_, mfid)| mfid)
                .ok_or_else(|| anyhow::anyhow!("manifest for restricted/dir not found"))?
        };

        let repo_ctx = RepoContext::new_test(ctx, Arc::new(repo)).await?;
        let hg = repo_ctx.hg();

        let restriction_ctx =
            HgAugmentedTreeRestrictionContext::new(hg, restricted_mfid.into()).await?;
        // Override JK to disable the check_permission endpoint
        let result = with_just_knobs_async(
            JustKnobsInMemory::new(hashmap! {
                "scm/mononoke:enable_server_side_path_acls".to_string() => KnobVal::Bool(false),
            }),
            async move { restriction_ctx.restriction_info().await }.boxed(),
        )
        .await;

        assert!(
            matches!(result, Err(MononokeError::NotAvailable(_))),
            "expected NotAvailable error when JK is disabled, got: {:?}",
            result
        );

        Ok(())
    }
}
