/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod delete;
mod list;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use repo_identity::RepoIdentity;
use restricted_paths::RestrictedManifestId;
use restricted_paths::RestrictedPathsManifestIdStore;
use smallvec::SmallVec;

/// Inspect and manage restricted paths state for a repo.
///
/// Subcommands are grouped by the underlying state they operate on (e.g.
/// `manifest-id-store`), leaving room for future restricted-paths tooling
/// (backfills, path lookups, etc.) that is unrelated to the manifest-id store.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: RestrictedPathsSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    restricted_paths_manifest_id_store: dyn RestrictedPathsManifestIdStore,
    #[facet]
    repo_identity: RepoIdentity,
}

#[derive(Subcommand)]
pub enum RestrictedPathsSubcommand {
    /// Inspect and manage the manifest-id store.
    ManifestIdStore {
        #[clap(subcommand)]
        command: ManifestIdStoreSubcommand,
    },
}

#[derive(Subcommand)]
pub enum ManifestIdStoreSubcommand {
    /// List the entries matching a manifest id in the selected repo.
    List(list::ListArgs),
    /// Delete the entries matching a manifest id in the selected repo.
    Delete(delete::DeleteArgs),
}

/// Parse a manifest id from its hex representation.
///
/// Accepts an optional `0x`/`0X` prefix. The bytes are decoded with strict hex
/// parsing rather than `RestrictedManifestId::from(&str)`, whose fallback silently treats
/// invalid hex as raw ASCII bytes and would mask user mistakes.
pub(crate) fn parse_manifest_id(s: &str) -> Result<RestrictedManifestId> {
    let hex_str = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    let bytes = match hex::decode(hex_str) {
        Ok(bytes) => bytes,
        Err(e) => bail!("Invalid hex manifest_id {s:?}: {e}"),
    };
    if bytes.is_empty() {
        bail!("Empty manifest_id {s:?}: a manifest id must be a non-empty hex string");
    }
    Ok(RestrictedManifestId::new(SmallVec::from_slice(&bytes)))
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    match args.subcommand {
        RestrictedPathsSubcommand::ManifestIdStore { command } => match command {
            ManifestIdStoreSubcommand::List(args) => list::list(&ctx, &repo, args).await?,
            ManifestIdStoreSubcommand::Delete(args) => delete::delete(&ctx, &repo, args).await?,
        },
    }
    Ok(())
}
