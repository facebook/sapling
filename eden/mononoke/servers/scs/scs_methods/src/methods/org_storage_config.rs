/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! An org's own storage engine, as a data file SCS lands for the repo portal.
//!
//! `scm/mononoke/repos/org_storage/<org>.json` holds the org's bucket and
//! shard; configerator builds the `org_<org>` engine from it and lists the
//! shard in the Mononoke schema domains (`org_storage.cinc`). This service
//! owns the land because the directory sits under the config ACL it already
//! lands RepoSpecs with. It is stateless: `create` is "ensure the file at
//! HEAD says this", `delete` is "ensure it is absent", the poll follows a
//! mutation, and `org_storage_config` reads the loaded map. Waiting, retries
//! and the human-facing steps stay with the caller.

use std::collections::HashSet;

#[cfg(fbcode_build)]
use configo::ConfigoClient;
use configo_thrift_srclients::make_ConfigoService_srclient;
use configo_thrift_srclients::thrift::Mutation;
use configo_thrift_srclients::thrift::MutationState;
use context::CoreContext;
use git_source_of_truth::GitSourceOfTruthConfig;
use metaconfig_types::BlobConfig;
use metaconfig_types::DatabaseConfig;
use metaconfig_types::MetadataDatabaseConfig;
use metaconfig_types::ShardableRemoteDatabaseConfig;
use metaconfig_types::ShardedDatabaseConfig;
use metaconfig_types::StorageConfig;
use mononoke_configs::MononokeConfigs;
use permission_checker::AclProvider;
use regex::Regex;
use source_control as thrift;
use tracing::info;
use tracing::warn;

use crate::methods::create_repos::DIFF_AUTHOR;
use crate::methods::create_repos::ORG_STORAGE_PREFIX;
#[cfg(fbcode_build)]
use crate::methods::create_repos::initiate_land_for_mutation;
use crate::source_control_impl::SourceControlServiceImpl;

/// Kill switch for the org storage endpoints. Off refuses `create` and
/// `delete`; the reads stay available.
const ORG_STORAGE_CONFIG_JK: &str = "scm/mononoke:create_org_storage_config_enabled";
/// Group whose members may land or remove an org's storage engine. Expected
/// member: the repo portal's service identity, which lands an engine only at
/// the end of an approved org onboarding. Source Control admins are also
/// accepted, and logged. Creating repos needs no membership here: a repo's
/// engine follows from its org.
const ORG_STORAGE_SETTERS_GROUP: &str = "repo_portal_storage_setters";
const ORG_STORAGE_DIR: &str = "source/scm/mononoke/repos/org_storage";

/// The data file, byte for byte what configerator parses. Keys sorted, so
/// two lands of the same org compare equal at HEAD.
fn data_file(org: &str, bucket: &str, shard: &str) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "engine": format!("{ORG_STORAGE_PREFIX}{org}"),
        "manifold_bucket": bucket,
        "xdb_shard": shard,
    }))
    .expect("three string fields always serialise")
}

fn data_file_path(org: &str) -> String {
    format!("{ORG_STORAGE_DIR}/{org}.json")
}

fn engine_name(org: &str) -> String {
    format!("{ORG_STORAGE_PREFIX}{org}")
}

/// Only mutations this service prepared for this directory may be landed
/// through the poll: a prepared-but-unreviewed human mutation elsewhere under
/// `scm/mononoke` must not become landable by anyone who can call SCS.
fn mutation_is_ours(mutation: &Mutation) -> bool {
    let author_ok = mutation
        .mutationInfo
        .as_ref()
        .and_then(|info| info.author.as_deref())
        == Some(DIFF_AUTHOR);
    let prefix = format!("{ORG_STORAGE_DIR}/");
    let paths = mutation
        .modifiedFilenames
        .iter()
        .flatten()
        .chain(mutation.deletedFilenames.iter().flatten())
        .collect::<Vec<_>>();
    author_ok && !paths.is_empty() && paths.iter().all(|path| path.starts_with(&prefix))
}

/// Mirrors `SLUG_RE` in `org_storage.cinc`: a dotted basename is not a safe
/// `__files__` key, so the portal refuses dots in a dedicated-storage slug.
fn validate_org(org: &str) -> Result<(), scs_errors::ServiceError> {
    let re = Regex::new(r"^[a-z0-9][a-z0-9_-]*$").expect("static regex");
    if org.is_empty() || !re.is_match(org) || org.len() > 64 {
        return Err(scs_errors::invalid_request(format!(
            "ORG_STORAGE_REFUSED:org: '{org}' is not an org slug (lowercase letters, digits, \
             _ and -, no dots, at most 64 characters)"
        ))
        .into());
    }
    Ok(())
}

/// Manifold's own bucket rule, as `org_storage.cinc` checks it.
fn validate_bucket(bucket: &str) -> Result<(), scs_errors::ServiceError> {
    let re = Regex::new(r"^[a-z0-9\-_.]+$").expect("static regex");
    if bucket == "-" || !re.is_match(bucket) {
        return Err(scs_errors::invalid_request(format!(
            "ORG_STORAGE_REFUSED:bucket: '{bucket}' is not a Manifold bucket name"
        ))
        .into());
    }
    Ok(())
}

fn validate_shard(shard: &str) -> Result<(), scs_errors::ServiceError> {
    let re = Regex::new(r"^xdb\.[a-z0-9_]{3,60}$").expect("static regex");
    if !re.is_match(shard) {
        return Err(scs_errors::invalid_request(format!(
            "ORG_STORAGE_REFUSED:shard: '{shard}' is not an XDB shard name (xdb.<name>)"
        ))
        .into());
    }
    if shard.starts_with("xdb.mononoke_") {
        return Err(scs_errors::invalid_request(format!(
            "ORG_STORAGE_REFUSED:shard: '{shard}' is a Source Control shard"
        ))
        .into());
    }
    Ok(())
}

/// Every Manifold bucket and every database or shard map a storage engine
/// names, walked structurally so a new engine or field in `storage.cinc` is
/// covered without a list here. The same walk `org_storage.cinc` does at
/// build time, run at request time so the refusal is a request error with
/// the asset named, not a failed mutation an hour later.
#[derive(Default)]
struct StorageAssets {
    buckets: HashSet<String>,
    databases: HashSet<String>,
}

impl StorageAssets {
    /// Every engine but the org's own (a re-land of the same file compares
    /// equal at HEAD before this matters; a different bucket for the same
    /// org is a conflict the file check reports).
    fn of_tier(configs: &MononokeConfigs, own_engine: &str) -> Self {
        let mut assets = Self::default();
        for (name, config) in configs.storage_configs().storage.iter() {
            if name == own_engine {
                continue;
            }
            assets.add_storage(config);
        }
        assets
    }

    fn add_storage(&mut self, config: &StorageConfig) {
        self.add_blob(&config.blobstore);
        self.add_blob(&config.mutable_blobstore);
        if let Some(ephemeral) = &config.ephemeral_blobstore {
            self.add_blob(&ephemeral.blobstore);
            if let DatabaseConfig::Remote(remote) = &ephemeral.metadata {
                self.databases.insert(remote.db_address.clone());
            }
        }
        if let MetadataDatabaseConfig::Remote(remote) = &config.metadata {
            for db in [
                &remote.production,
                &remote.commit_graph,
                &remote.bookmarks,
                &remote.bonsai_hg_mapping,
                &remote.mutation,
                &remote.sparse_profiles,
            ] {
                self.databases.insert(db.db_address.clone());
            }
            for db in [
                &remote.deletion_log,
                &remote.commit_cloud,
                &remote.git_bundle_metadata,
                &remote.repo_metadata,
                &remote.restricted_paths,
                &remote.repo_manifest_mapping,
            ]
            .into_iter()
            .flatten()
            {
                self.databases.insert(db.db_address.clone());
            }
            self.add_shardable(&remote.filenodes);
            for db in [
                &remote.bonsai_blob_mapping,
                &remote.commit_derived_data_mapping,
            ]
            .into_iter()
            .flatten()
            {
                self.add_shardable(db);
            }
        }
    }

    fn add_shardable(&mut self, db: &ShardableRemoteDatabaseConfig) {
        match db {
            ShardableRemoteDatabaseConfig::Unsharded(remote) => {
                self.databases.insert(remote.db_address.clone());
            }
            ShardableRemoteDatabaseConfig::Sharded(sharded) => {
                self.databases.insert(sharded.shard_map.clone());
            }
        }
    }

    fn add_blob(&mut self, blob: &BlobConfig) {
        match blob {
            BlobConfig::Manifold { bucket, .. } | BlobConfig::ManifoldWithTtl { bucket, .. } => {
                self.buckets.insert(bucket.clone());
            }
            BlobConfig::Mysql { remote } => self.add_shardable(remote),
            BlobConfig::MultiplexedWal {
                blobstores,
                queue_db,
                ..
            } => {
                match queue_db {
                    ShardedDatabaseConfig::Sharded(sharded) => {
                        self.databases.insert(sharded.shard_map.clone());
                    }
                    ShardedDatabaseConfig::Unsharded(remote) => {
                        self.databases.insert(remote.db_address.clone());
                    }
                    ShardedDatabaseConfig::Local(_) => {}
                }
                for (_id, _store_type, inner) in blobstores {
                    self.add_blob(inner);
                }
            }
            BlobConfig::Logging { blobconfig, .. } | BlobConfig::Pack { blobconfig, .. } => {
                self.add_blob(blobconfig)
            }
            BlobConfig::Disabled
            | BlobConfig::Files { .. }
            | BlobConfig::Sqlite { .. }
            | BlobConfig::AwsS3 { .. } => {}
        }
    }
}

/// Everything about a create that is decided before Configo is touched.
fn validate_create(
    params: &thrift::CreateOrgStorageConfigParams,
    configs: &MononokeConfigs,
) -> Result<(), scs_errors::ServiceError> {
    validate_org(&params.org)?;
    validate_bucket(&params.manifold_bucket)?;
    validate_shard(&params.xdb_shard)?;
    let taken = StorageAssets::of_tier(configs, &engine_name(&params.org));
    if taken.buckets.contains(&params.manifold_bucket) {
        return Err(scs_errors::invalid_request(format!(
            "ORG_STORAGE_REFUSED:bucket: '{}' belongs to another storage engine",
            params.manifold_bucket
        ))
        .into());
    }
    if taken.databases.contains(&params.xdb_shard) {
        return Err(scs_errors::invalid_request(format!(
            "ORG_STORAGE_REFUSED:shard: '{}' belongs to another storage engine",
            params.xdb_shard
        ))
        .into());
    }
    Ok(())
}

/// Caller gate for landing or removing an engine: a property of the
/// caller's identities, never of the `client_id` header. Fails closed: a
/// group lookup error refuses.
async fn ensure_caller_may_manage_org_storage(
    ctx: &CoreContext,
    acl_provider: &dyn AclProvider,
    method: &str,
    engine: &str,
) -> Result<(), scs_errors::ServiceError> {
    let identities = ctx.metadata().identities();
    let setters = acl_provider
        .group(ORG_STORAGE_SETTERS_GROUP)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    if setters.is_member(identities).await {
        return Ok(());
    }
    let admins = acl_provider
        .admin_group()
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    if admins.is_member(identities).await {
        warn!(
            method,
            storage_config = engine,
            "org storage engine managed by a Source Control admin outside \
             {ORG_STORAGE_SETTERS_GROUP} (break-glass)"
        );
        let mut scuba = ctx.scuba().clone();
        scuba.add("action", "org_storage_break_glass");
        scuba.add("method", method);
        scuba.add("storage_config", engine);
        scuba.log_with_msg("org storage engine managed by an admin (break-glass)", None);
        return Ok(());
    }
    Err(scs_errors::invalid_request(format!(
        "ORG_STORAGE_REFUSED:caller: {method} needs membership of {ORG_STORAGE_SETTERS_GROUP}"
    ))
    .into())
}

fn knob_on() -> Result<(), scs_errors::ServiceError> {
    if !justknobs::eval(ORG_STORAGE_CONFIG_JK, None, None) {
        return Err(scs_errors::invalid_request(format!(
            "ORG_STORAGE_DISABLED: org storage config lands are switched off ({ORG_STORAGE_CONFIG_JK})"
        ))
        .into());
    }
    Ok(())
}

/// Repos that name the engine, from both places a repo can exist: the
/// source-of-truth table (any state, the row is the repo's existence) and
/// the loaded repo configs (a repo whose spec failed to parse is in neither,
/// which is why the name prefix is checked too).
async fn repos_on_engine(
    ctx: &CoreContext,
    org: &str,
    configs: &MononokeConfigs,
    git_source_of_truth_config: &dyn GitSourceOfTruthConfig,
) -> Result<Vec<String>, scs_errors::ServiceError> {
    let engine = engine_name(org);
    let prefix = format!("{org}/");
    let mut names = Vec::new();
    for entry in git_source_of_truth_config
        .get_any(ctx)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?
    {
        if entry.storage_config.as_deref() == Some(engine.as_str())
            || entry.repo_name.0.starts_with(&prefix)
        {
            names.push(entry.repo_name.0);
        }
    }
    let storage_configs = configs.storage_configs();
    if let Some(engine_config) = storage_configs.storage.get(&engine) {
        for (name, config) in configs.repo_configs().repos.iter() {
            if config.storage_config == *engine_config && !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    names.sort();
    Ok(names)
}

impl SourceControlServiceImpl {
    pub(crate) async fn create_org_storage_config(
        &self,
        ctx: CoreContext,
        params: thrift::CreateOrgStorageConfigParams,
    ) -> Result<thrift::CreateOrgStorageConfigToken, scs_errors::ServiceError> {
        knob_on()?;
        validate_create(&params, &self.configs)?;
        ensure_caller_may_manage_org_storage(
            &ctx,
            self.acl_provider.as_ref(),
            "create_org_storage_config",
            &engine_name(&params.org),
        )
        .await?;
        create_impl(ctx, params).await
    }

    pub(crate) async fn delete_org_storage_config(
        &self,
        ctx: CoreContext,
        params: thrift::DeleteOrgStorageConfigParams,
    ) -> Result<thrift::DeleteOrgStorageConfigToken, scs_errors::ServiceError> {
        knob_on()?;
        validate_org(&params.org)?;
        ensure_caller_may_manage_org_storage(
            &ctx,
            self.acl_provider.as_ref(),
            "delete_org_storage_config",
            &engine_name(&params.org),
        )
        .await?;
        let in_use = repos_on_engine(
            &ctx,
            &params.org,
            &self.configs,
            self.git_source_of_truth_config.as_ref(),
        )
        .await?;
        if !in_use.is_empty() {
            return Err(scs_errors::invalid_request(format!(
                "ORG_STORAGE_IN_USE: {} repo(s) exist under '{}/' or on engine '{}{}': {}",
                in_use.len(),
                params.org,
                ORG_STORAGE_PREFIX,
                params.org,
                in_use.join(", "),
            ))
            .into());
        }
        delete_impl(ctx, params).await
    }

    pub(crate) async fn org_storage_config_poll(
        &self,
        ctx: CoreContext,
        params: thrift::OrgStorageConfigPollParams,
    ) -> Result<thrift::OrgStorageConfigPollResponse, scs_errors::ServiceError> {
        poll_impl(ctx, params.mutation_id).await
    }

    pub(crate) async fn org_storage_config(
        &self,
        _ctx: CoreContext,
        params: thrift::OrgStorageConfigParams,
    ) -> Result<thrift::OrgStorageConfigResponse, scs_errors::ServiceError> {
        validate_org(&params.org)?;
        let engine = engine_name(&params.org);
        let loaded = self.configs.storage_configs().storage.contains_key(&engine);
        Ok(thrift::OrgStorageConfigResponse {
            storage_config: loaded.then_some(engine),
            manifest_version: self.configs.manifest_version(),
            ..Default::default()
        })
    }
}

#[cfg(fbcode_build)]
async fn create_impl(
    ctx: CoreContext,
    params: thrift::CreateOrgStorageConfigParams,
) -> Result<thrift::CreateOrgStorageConfigToken, scs_errors::ServiceError> {
    let configo_client = ConfigoClient::with_client(
        ctx.fb,
        make_ConfigoService_srclient!(ctx.fb)
            .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?,
    );
    let mut txn = configo_client.managed_transaction();
    let path = data_file_path(&params.org);
    let wanted = data_file(&params.org, &params.manifold_bucket, &params.xdb_shard);
    // Reading pins the transaction to a version, so a concurrent land of the
    // same path makes this mutation STALE rather than silently last-wins.
    match txn.get_file(path.clone()).await {
        Ok(current) => {
            if *current == wanted {
                return Ok(thrift::CreateOrgStorageConfigToken {
                    mutation_id: None,
                    already_landed: true,
                    ..Default::default()
                });
            }
            return Err(scs_errors::invalid_request(format!(
                "ORG_STORAGE_CONFLICT:{path}: a file for org '{}' is already landed with different \
                 content; it is never overwritten",
                params.org
            ))
            .into());
        }
        Err(configo::ConfigoError::MissingFile(_)) => {}
        Err(e) => {
            return Err(scs_errors::internal_error(format!("{e:#}")).into());
        }
    }
    txn.set_file(path, wanted);
    let mutation = txn
        .prepare_mutation_request()
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?
        .add_author(DIFF_AUTHOR.to_string())
        .add_commit_message(
            format!(
                "[mononoke]: Add storage engine {ORG_STORAGE_PREFIX}{} (automated)",
                params.org
            ),
            format!(
                "Org {} on bucket {} and shard {}, landed by SCS for the repo portal.",
                params.org, params.manifold_bucket, params.xdb_shard
            ),
        )
        .prepare_nowait()
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    info!(
        org = params.org,
        mutation_id = mutation.id,
        "create_org_storage_config prepared a mutation"
    );
    let mut scuba = ctx.scuba().clone();
    scuba.add("action", "create_org_storage_config_mutation_prepared");
    scuba.add("org_slug", params.org.clone());
    scuba.add(
        "storage_config",
        format!("{ORG_STORAGE_PREFIX}{}", params.org),
    );
    scuba.add("manifold_bucket", params.manifold_bucket.clone());
    scuba.add("xdb_shard", params.xdb_shard.clone());
    scuba.add("mutation_id", mutation.id);
    scuba.log_with_msg("create_org_storage_config prepared a mutation", None);
    Ok(thrift::CreateOrgStorageConfigToken {
        mutation_id: Some(mutation.id),
        already_landed: false,
        ..Default::default()
    })
}

#[cfg(fbcode_build)]
async fn delete_impl(
    ctx: CoreContext,
    params: thrift::DeleteOrgStorageConfigParams,
) -> Result<thrift::DeleteOrgStorageConfigToken, scs_errors::ServiceError> {
    let configo_client = ConfigoClient::with_client(
        ctx.fb,
        make_ConfigoService_srclient!(ctx.fb)
            .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?,
    );
    let mut txn = configo_client.managed_transaction();
    let path = data_file_path(&params.org);
    match txn.get_file(path.clone()).await {
        Ok(_) => {}
        Err(configo::ConfigoError::MissingFile(_)) => {
            return Ok(thrift::DeleteOrgStorageConfigToken {
                mutation_id: None,
                already_absent: true,
                ..Default::default()
            });
        }
        Err(e) => {
            return Err(scs_errors::internal_error(format!("{e:#}")).into());
        }
    }
    txn.mark_file_for_deletion(path);
    let mutation = txn
        .prepare_mutation_request()
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?
        .add_author(DIFF_AUTHOR.to_string())
        .add_commit_message(
            format!(
                "[mononoke]: Remove storage engine {ORG_STORAGE_PREFIX}{} (automated)",
                params.org
            ),
            format!(
                "Org {} left dedicated storage with no repos on it; removed by SCS for the repo portal.",
                params.org
            ),
        )
        .prepare_nowait()
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    let mut scuba = ctx.scuba().clone();
    scuba.add("action", "delete_org_storage_config_mutation_prepared");
    scuba.add("org_slug", params.org.clone());
    scuba.add("mutation_id", mutation.id);
    scuba.log_with_msg("delete_org_storage_config prepared a mutation", None);
    Ok(thrift::DeleteOrgStorageConfigToken {
        mutation_id: Some(mutation.id),
        already_absent: false,
        ..Default::default()
    })
}

/// Follows a mutation the way `create_repos_poll` does, minus the cleanup:
/// nothing was reserved, so a FAILED or ABORTED mutation is reported and the
/// caller decides whether to prepare another.
#[cfg(fbcode_build)]
async fn poll_impl(
    ctx: CoreContext,
    mutation_id: i64,
) -> Result<thrift::OrgStorageConfigPollResponse, scs_errors::ServiceError> {
    let configo_client = make_ConfigoService_srclient!(ctx.fb)
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    let resp = configo_client
        .status(&mutation_id)
        .await
        .map_err(|e| scs_errors::internal_error(format!("{e:#}")))?;
    if resp.error {
        return Ok(thrift::OrgStorageConfigPollResponse {
            state: thrift::OrgStorageConfigMutationState::FAILED,
            message: Some(resp.errorMessage),
            ..Default::default()
        });
    }
    let mutation = resp.mutation.ok_or_else(|| {
        scs_errors::internal_error(format!("Mutation state not available for {mutation_id}"))
    })?;
    if mutation.stateInfo.isError {
        return Ok(thrift::OrgStorageConfigPollResponse {
            state: thrift::OrgStorageConfigMutationState::FAILED,
            message: Some(mutation.stateInfo.errorMessage),
            ..Default::default()
        });
    }
    if !mutation_is_ours(&mutation) {
        return Err(scs_errors::invalid_request(format!(
            "ORG_STORAGE_REFUSED:mutation: {mutation_id} was not prepared by this service for \
             {ORG_STORAGE_DIR}"
        ))
        .into());
    }
    let state = match mutation.stateInfo.state {
        MutationState::LANDED => thrift::OrgStorageConfigMutationState::LANDED,
        MutationState::FAILED => thrift::OrgStorageConfigMutationState::FAILED,
        MutationState::ABORTED => thrift::OrgStorageConfigMutationState::ABORTED,
        MutationState::PREPARED => {
            if !mutation.stateInfo.isSigned {
                initiate_land_for_mutation(ctx, configo_client, mutation_id).await?;
            }
            thrift::OrgStorageConfigMutationState::IN_PROGRESS
        }
        MutationState::CANARYING
        | MutationState::PREPARING
        | MutationState::LANDING
        | MutationState::SERVICE_CANARYING
        | MutationState::VALIDATING => thrift::OrgStorageConfigMutationState::IN_PROGRESS,
        other => {
            return Err(scs_errors::internal_error(format!(
                "Unexpected Configo mutation state: {other}"
            ))
            .into());
        }
    };
    Ok(thrift::OrgStorageConfigPollResponse {
        state,
        message: None,
        ..Default::default()
    })
}

#[cfg(not(fbcode_build))]
async fn create_impl(
    _ctx: CoreContext,
    _params: thrift::CreateOrgStorageConfigParams,
) -> Result<thrift::CreateOrgStorageConfigToken, scs_errors::ServiceError> {
    Err(scs_errors::internal_error("No access to configo in oss build".to_string()).into())
}

#[cfg(not(fbcode_build))]
async fn delete_impl(
    _ctx: CoreContext,
    _params: thrift::DeleteOrgStorageConfigParams,
) -> Result<thrift::DeleteOrgStorageConfigToken, scs_errors::ServiceError> {
    Err(scs_errors::internal_error("No access to configo in oss build".to_string()).into())
}

#[cfg(not(fbcode_build))]
async fn poll_impl(
    _ctx: CoreContext,
    _mutation_id: i64,
) -> Result<thrift::OrgStorageConfigPollResponse, scs_errors::ServiceError> {
    Err(scs_errors::internal_error("No access to configo in oss build".to_string()).into())
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    fn request_error(err: scs_errors::ServiceError) -> String {
        match err {
            scs_errors::ServiceError::Request(e) => e.reason,
            other => panic!("expected a request error, got {other:?}"),
        }
    }

    #[mononoke::test]
    fn test_data_file_is_the_configerator_shape() {
        let bytes = data_file("wearables-fw", "wearables_fw_mononoke", "xdb.wearables_fw");
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!({
                "engine": "org_wearables-fw",
                "manifold_bucket": "wearables_fw_mononoke",
                "xdb_shard": "xdb.wearables_fw",
            })
        );
        assert_eq!(
            data_file_path("wearables-fw"),
            "source/scm/mononoke/repos/org_storage/wearables-fw.json"
        );
        // Deterministic: the idempotency check compares bytes at HEAD.
        assert_eq!(
            bytes,
            data_file("wearables-fw", "wearables_fw_mononoke", "xdb.wearables_fw")
        );
    }

    fn mutation(author: Option<&str>, modified: &[&str], deleted: &[&str]) -> Mutation {
        Mutation {
            mutationInfo: Some(configo_thrift_srclients::thrift::MutationInfo {
                author: author.map(str::to_string),
                ..Default::default()
            }),
            modifiedFilenames: Some(modified.iter().map(|s| s.to_string()).collect()),
            deletedFilenames: Some(deleted.iter().map(|s| s.to_string()).collect()),
            ..Default::default()
        }
    }

    // The poll lands only what this service prepared for its own directory.
    #[mononoke::test]
    fn test_poll_lands_only_our_mutations() {
        let ours = "source/scm/mononoke/repos/org_storage/x.json";
        assert!(mutation_is_ours(&mutation(Some(DIFF_AUTHOR), &[ours], &[])));
        assert!(mutation_is_ours(&mutation(Some(DIFF_AUTHOR), &[], &[ours])));
        assert!(!mutation_is_ours(&mutation(Some("someone"), &[ours], &[])));
        assert!(!mutation_is_ours(&mutation(None, &[ours], &[])));
        assert!(!mutation_is_ours(&mutation(Some(DIFF_AUTHOR), &[], &[])));
        assert!(!mutation_is_ours(&mutation(
            Some(DIFF_AUTHOR),
            &[ours, "source/scm/mononoke/repos/git/ab/x.cconf"],
            &[]
        )));
        assert!(!mutation_is_ours(&mutation(
            Some(DIFF_AUTHOR),
            &["source/scm/mononoke/repos/org_storage_other/x.json"],
            &[]
        )));
    }

    #[mononoke::test]
    fn test_names_are_validated_like_the_cinc() {
        validate_org("wearables-fw").unwrap();
        validate_org("a_1").unwrap();
        for bad in [
            "",
            "Fw",
            "fw.malibu",
            "a/b",
            "-x",
            "(none)",
            &"a".repeat(65),
        ] {
            let reason = request_error(validate_org(bad).unwrap_err());
            assert!(
                reason.starts_with("ORG_STORAGE_REFUSED:org"),
                "{bad}: {reason}"
            );
        }
        validate_bucket("wearables_fw_mononoke").unwrap();
        for bad in ["-", "Bad Bucket", "bucket/x"] {
            let reason = request_error(validate_bucket(bad).unwrap_err());
            assert!(
                reason.starts_with("ORG_STORAGE_REFUSED:bucket"),
                "{bad}: {reason}"
            );
        }
        validate_shard("xdb.wearables_fw").unwrap();
        for bad in ["wearables", "xdb.ab", "xdb.mononoke_bookmarks", "xdb.A"] {
            let reason = request_error(validate_shard(bad).unwrap_err());
            assert!(
                reason.starts_with("ORG_STORAGE_REFUSED:shard"),
                "{bad}: {reason}"
            );
        }
    }

    fn remote(address: &str) -> metaconfig_types::RemoteDatabaseConfig {
        metaconfig_types::RemoteDatabaseConfig {
            db_address: address.to_string(),
        }
    }

    // The structural walk sees the assets a hand list would miss: the
    // ephemeral store's bucket and db, the WAL queue, the cross-repo dbs.
    #[mononoke::test]
    fn test_storage_assets_walk_every_field() {
        let config = StorageConfig {
            blobstore: BlobConfig::Logging {
                blobconfig: Box::new(BlobConfig::MultiplexedWal {
                    multiplex_id: metaconfig_types::MultiplexId::new(1),
                    blobstores: vec![
                        (
                            metaconfig_types::BlobstoreId::new(1),
                            metaconfig_types::MultiplexedStoreType::Normal,
                            BlobConfig::Manifold {
                                bucket: "mononoke_prod".to_string(),
                                prefix: String::new(),
                            },
                        ),
                        (
                            metaconfig_types::BlobstoreId::new(2),
                            metaconfig_types::MultiplexedStoreType::Normal,
                            BlobConfig::Pack {
                                blobconfig: Box::new(BlobConfig::Mysql {
                                    remote: ShardableRemoteDatabaseConfig::Sharded(
                                        metaconfig_types::ShardedRemoteDatabaseConfig {
                                            shard_map: "xdb.mononoke_production_blobstore"
                                                .to_string(),
                                            shard_num: std::num::NonZeroUsize::new(4).unwrap(),
                                        },
                                    ),
                                }),
                                pack_config: None,
                            },
                        ),
                    ],
                    write_quorum: 1,
                    queue_db: ShardedDatabaseConfig::Sharded(
                        metaconfig_types::ShardedRemoteDatabaseConfig {
                            shard_map: "xdb.mononoke_blobstore_wal_queue".to_string(),
                            shard_num: std::num::NonZeroUsize::new(8).unwrap(),
                        },
                    ),
                    inner_blobstores_scuba_table: None,
                    multiplex_scuba_table: None,
                    scuba_sample_rate: std::num::NonZeroU64::new(1).unwrap(),
                }),
                scuba_table: None,
                scuba_sample_rate: std::num::NonZeroU64::new(1).unwrap(),
            },
            mutable_blobstore: BlobConfig::ManifoldWithTtl {
                bucket: "mononoke_mutable".to_string(),
                prefix: String::new(),
                ttl: std::time::Duration::from_secs(1),
            },
            ephemeral_blobstore: Some(metaconfig_types::EphemeralBlobstoreConfig {
                blobstore: BlobConfig::Manifold {
                    bucket: "mononoke_snapshots".to_string(),
                    prefix: String::new(),
                },
                metadata: DatabaseConfig::Remote(remote("xdb.mononoke_snapshots")),
                initial_bubble_lifespan: std::time::Duration::from_secs(1),
                bubble_expiration_grace: std::time::Duration::from_secs(1),
                bubble_deletion_mode: metaconfig_types::BubbleDeletionMode::Disabled,
            }),
            metadata: MetadataDatabaseConfig::Remote(
                metaconfig_types::RemoteMetadataDatabaseConfig {
                    production: remote("xdb.mononoke_production"),
                    commit_graph: remote("xdb.mononoke_commit_graph"),
                    bookmarks: remote("xdb.mononoke_bookmarks"),
                    bonsai_hg_mapping: remote("xdb.mononoke_bonsai_hg_mapping"),
                    filenodes: ShardableRemoteDatabaseConfig::Sharded(
                        metaconfig_types::ShardedRemoteDatabaseConfig {
                            shard_map: "xdb.mononoke_production_filenodes".to_string(),
                            shard_num: std::num::NonZeroUsize::new(60).unwrap(),
                        },
                    ),
                    mutation: remote("xdb.mononoke_mutation"),
                    sparse_profiles: remote("xdb.sparse_profiles_sizes"),
                    bonsai_blob_mapping: None,
                    deletion_log: Some(remote("xdb.mononoke_deletion")),
                    commit_cloud: Some(remote("xdb.commit_cloud")),
                    git_bundle_metadata: Some(remote("xdb.git_bundle_metadata")),
                    repo_metadata: None,
                    restricted_paths: None,
                    commit_derived_data_mapping: None,
                    repo_manifest_mapping: None,
                },
            ),
        };
        let mut assets = StorageAssets::default();
        assets.add_storage(&config);
        for bucket in ["mononoke_prod", "mononoke_mutable", "mononoke_snapshots"] {
            assert!(assets.buckets.contains(bucket), "{bucket}");
        }
        for db in [
            "xdb.mononoke_production_blobstore",
            "xdb.mononoke_blobstore_wal_queue",
            "xdb.mononoke_snapshots",
            "xdb.mononoke_production",
            "xdb.mononoke_production_filenodes",
            "xdb.sparse_profiles_sizes",
            "xdb.commit_cloud",
            "xdb.git_bundle_metadata",
            "xdb.mononoke_deletion",
        ] {
            assert!(assets.databases.contains(db), "{db}");
        }
    }
}
