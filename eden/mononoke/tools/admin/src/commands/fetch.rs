/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bonsai_p4_mapping::BonsaiP4Mapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use clap::ValueEnum;
use cmdlib_displaying::DisplayChangeset;
use cmdlib_displaying::display_content;
use cmdlib_displaying::display_hg_augmented_manifest;
use cmdlib_displaying::display_hg_manifest;
use cmdlib_displaying::display_manifest;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStore;
use ephemeral_blobstore::RepoEphemeralStoreRef;
use filestore::FetchKey;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgManifestId;
use mercurial_types::NULL_HASH;
use mercurial_types::blobs::hg_manifests_read_from_augmented;
use mercurial_types::sharded_augmented_manifest::hg_entry_from_augmented;
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;
use mononoke_types::ChangesetId;
use mononoke_types::path::MPath;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;

/// Fetch commit, tree or file data.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    /// Fetch from within this ephemeral bubble
    #[clap(long)]
    bubble_id: Option<BubbleId>,

    /// Path of the tree or file to fetch
    #[clap(long, short = 'p')]
    path: Option<String>,

    /// Format as JSON. Currently works only for changesets.
    #[clap(long)]
    json: bool,

    /// Manifest type to use to find trees or files.
    #[clap(long, short = 'k', value_enum, default_value_t = ManifestKind::Hg)]
    manifest_kind: ManifestKind,
}

#[facet::container]
pub struct Repo {
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    bonsai_p4_mapping: dyn BonsaiP4Mapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_ephemeral_store: RepoEphemeralStore,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_identity: RepoIdentity,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, ValueEnum)]
pub enum ManifestKind {
    Hg,
    ContentManifest,
    // TODO: Add unode manifest and skmf support
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let repo: Repo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    let changeset_id = args
        .changeset_args
        .resolve_changeset(&ctx, &repo)
        .await
        .context("Failed to resolve changeset")?;

    let blobstore = match args.bubble_id {
        None => repo.repo_blobstore().clone(),
        Some(bubble_id) => {
            let bubble = repo
                .repo_ephemeral_store()
                .open_bubble(&ctx, bubble_id)
                .await
                .with_context(|| format!("Failed to open bubble {bubble_id}"))?;
            bubble.wrap_repo_blobstore(repo.repo_blobstore().clone())
        }
    };

    match &args.path {
        None => {
            let cs = changeset_id
                .load(&ctx, &blobstore)
                .await
                .with_context(|| format!("Failed to load changeset {changeset_id}"))?;
            let display_cs =
                DisplayChangeset::try_from(&cs).context("Failed to display changeset")?;

            if args.json {
                let json_cs =
                    serde_json::to_string(&display_cs).context("Failed to convert to JSON")?;
                println!("{json_cs}");
            } else {
                println!("{display_cs}");
            }
        }

        Some(path) => match args.manifest_kind {
            ManifestKind::Hg => {
                let hg_changeset_id = repo
                    .bonsai_hg_mapping()
                    .get_hg_from_bonsai(&ctx, changeset_id)
                    .await
                    .context("Failed to get corresponding Hg changeset")?
                    .ok_or_else(|| anyhow!("No Hg changeset for {changeset_id}"))?;
                let root = hg_changeset_id
                    .load(&ctx, &blobstore)
                    .await
                    .context("Failed to load Hg changeset")?
                    .manifestid();
                let read_from_augmented =
                    hg_manifests_read_from_augmented(repo.repo_identity().name());
                display_hg_entry(&ctx, &blobstore, root, path, read_from_augmented).await?;
            }
            ManifestKind::ContentManifest => {
                display_content_manifest(&ctx, &repo, &blobstore, changeset_id, path).await?;
            }
        },
    }

    Ok(())
}

async fn display_content_info(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    fetch_key: &FetchKey,
) -> Result<()> {
    let metadata = filestore::get_metadata(blobstore, ctx, fetch_key)
        .await
        .context("Failed to load metadata")?
        .ok_or_else(|| anyhow!("Content for file {fetch_key} not found"))?;
    writeln!(std::io::stdout(), "Size: {}", metadata.total_size)?;
    writeln!(std::io::stdout(), "Content-Id: {}", metadata.content_id)?;
    writeln!(std::io::stdout(), "Sha1: {}", metadata.sha1)?;
    writeln!(std::io::stdout(), "Sha256: {}", metadata.sha256)?;
    writeln!(std::io::stdout(), "Git-Sha1: {}", metadata.git_sha1)?;

    let content = filestore::fetch_concat(blobstore, ctx, fetch_key.clone())
        .await
        .context("Failed to load content")?;
    display_content(std::io::stdout(), content)?;
    Ok(())
}

async fn display_content_manifest(
    ctx: &CoreContext,
    repo: &Repo,
    blobstore: &RepoBlobstore,
    cs_id: ChangesetId,
    path: &str,
) -> Result<()> {
    let root_manifest_id = repo
        .repo_derived_data()
        .derive::<RootContentManifestId>(ctx, cs_id, DerivationPriority::LOW)
        .await?
        .into_content_manifest_id();

    let entry = if path.is_empty() {
        Entry::Tree(root_manifest_id)
    } else {
        let mpath = MPath::new(path).with_context(|| format!("Invalid path: {path}"))?;
        root_manifest_id
            .find_entry(ctx.clone(), blobstore.clone(), mpath)
            .await?
            .ok_or_else(|| anyhow!("Path does not exist: {path}"))?
    };
    match entry {
        Entry::Leaf(file) => {
            writeln!(std::io::stdout(), "File-Type: {}", file.file_type)?;
            display_content_info(ctx, blobstore, &file.content_id.into()).await?;
        }
        Entry::Tree(id) => {
            let manifest = id
                .load(ctx, blobstore)
                .await
                .context("Failed to load content manifest")?;
            display_manifest(std::io::stdout(), ctx, blobstore, manifest).await?;
        }
    }
    Ok(())
}

async fn display_hg_entry(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    root: HgManifestId,
    path: &str,
    read_from_augmented: bool,
) -> Result<()> {
    // The null manifest id has no augmented manifest. Hg manifest reads treat
    // it as an empty tree without reading the blobstore.
    let read_from_augmented = read_from_augmented && root.into_nodehash() != NULL_HASH;
    let entry = if path.is_empty() {
        Entry::Tree(root)
    } else {
        let mpath = MPath::new(path).with_context(|| format!("Invalid path: {path}"))?;
        let entry = if read_from_augmented {
            HgAugmentedManifestId::from(root)
                .find_entry(ctx.clone(), blobstore.clone(), mpath)
                .await
                .map(|entry| entry.map(hg_entry_from_augmented))
        } else {
            root.find_entry(ctx.clone(), blobstore.clone(), mpath).await
        };
        entry
            .context("Failed to traverse manifest")?
            .ok_or_else(|| anyhow!("Path does not exist: {path}"))?
    };
    match entry {
        Entry::Leaf((file_type, id)) => {
            let envelope = id
                .load(ctx, blobstore)
                .await
                .context("Failed to load envelope")?;
            writeln!(std::io::stdout(), "File-Type: {file_type}")?;
            display_content_info(ctx, blobstore, &envelope.content_id().into()).await?;
        }
        Entry::Tree(id) if read_from_augmented => {
            let manifest = HgAugmentedManifestId::from(id)
                .load(ctx, blobstore)
                .await
                .context("Failed to load augmented manifest")?;
            display_hg_augmented_manifest(ctx, blobstore, std::io::stdout(), &manifest).await?;
        }
        Entry::Tree(id) => {
            let manifest = id
                .load(ctx, blobstore)
                .await
                .context("Failed to load manifest")?;
            display_hg_manifest(ctx, blobstore, std::io::stdout(), &manifest).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;

    use super::*;
    use crate::hg_manifest_test_fixture::HgManifestFixture;

    #[mononoke::fbinit_test]
    async fn test_display_hg_entry_from_augmented(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let f = HgManifestFixture::new(&ctx).await?;
        f.derive_augmented(&ctx, f.child).await?;
        let repo: Repo = f.repo().await?;
        let blobstore = repo.repo_blobstore();
        let root = f.child_mf;
        let mut hg_listing = Vec::new();
        let hg_manifest = root.load(&ctx, blobstore).await?;
        display_hg_manifest(&ctx, blobstore, &mut hg_listing, &hg_manifest).await?;

        // When: Hg manifests can no longer be read.
        f.blocker.arm();

        // Then: trees and files are still found and displayed from the
        // augmented manifest, and a tree's listing is unchanged.
        for path in ["", "dir", "dir/b", "e"] {
            display_hg_entry(&ctx, blobstore, root, path, true).await?;
        }
        let mut augmented_listing = Vec::new();
        let augmented_manifest = HgAugmentedManifestId::from(root)
            .load(&ctx, blobstore)
            .await?;
        display_hg_augmented_manifest(&ctx, blobstore, &mut augmented_listing, &augmented_manifest)
            .await?;
        assert_eq!(
            String::from_utf8(augmented_listing)?,
            String::from_utf8(hg_listing)?
        );
        assert_eq!(f.blocker.reads(), 0, "no Hg manifest is read");

        // And: reading Hg manifests still reads them.
        assert!(
            display_hg_entry(&ctx, blobstore, root, "dir", false)
                .await
                .is_err()
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_display_hg_entry_missing_augmented(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let f = HgManifestFixture::new(&ctx).await?;
        let repo: Repo = f.repo().await?;
        f.blocker.arm();

        let err = display_hg_entry(&ctx, repo.repo_blobstore(), f.child_mf, "dir", true)
            .await
            .expect_err("a missing augmented manifest is an error");
        assert!(
            format!("{err:#}").contains("hgaugmentedmanifest.sha1."),
            "the error names the missing augmented manifest: {err:#}"
        );
        assert_eq!(
            f.blocker.reads(),
            0,
            "there is no fallback to the Hg manifest"
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_display_hg_entry_null_manifest(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let f = HgManifestFixture::new(&ctx).await?;
        let repo: Repo = f.repo().await?;
        let blobstore = repo.repo_blobstore();
        let null = HgManifestId::new(NULL_HASH);

        // When: Hg manifests can no longer be read.
        f.blocker.arm();

        // Then: like an Hg manifest read, the null manifest is shown as an
        // empty tree, and has no other path, without reading any Hg manifest.
        display_hg_entry(&ctx, blobstore, null, "", true).await?;
        let err = display_hg_entry(&ctx, blobstore, null, "dir", true)
            .await
            .expect_err("the null manifest has no `dir`");
        assert_eq!(format!("{err:#}"), "Path does not exist: dir");
        assert_eq!(f.blocker.reads(), 0, "no Hg manifest is read");
        Ok(())
    }
}
