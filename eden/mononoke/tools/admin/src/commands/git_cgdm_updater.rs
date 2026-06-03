/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::KeyedBlobstore;
use blobstore::Storable;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMappingArc;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::Bookmarks;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use cloned::cloned;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use context::SessionClass;
use derivation_queue_thrift::DerivationPriority;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use git_types::CGDMCommitPackfileItems;
use git_types::CGDMComponents;
use git_types::CompactedGitDeltaManifest;
use git_types::ComponentInfo;
use git_types::GitDeltaManifestV3;
use git_types::RootGitDeltaManifestV3Id;
use itertools::Itertools;
use metaconfig_types::RepoConfig;
use metaconfig_types::RepoConfigRef;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeReposManager;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;
use mononoke_types::BlobstoreValue;
use mononoke_types::ThriftConvert;
use mutable_blobstore::MutableRepoBlobstore;
use mutable_blobstore::MutableRepoBlobstoreRef;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;

/// Create and update Compacted Git Delta Manifest for repos
#[derive(Parser)]
pub struct CommandArgs {
    /// The maximum number of changesets in a component.
    #[clap(long)]
    component_max_count: u64,

    /// The maximum size of a component blob in bytes.
    #[clap(long)]
    component_max_size: u64,

    /// Whether to re-index all commits or start
    /// from the previous blob.
    #[clap(long)]
    rebuild: bool,

    #[clap(subcommand)]
    mode: Mode,
}

#[derive(Subcommand)]
pub enum Mode {
    /// Update CGDM for a specific repo
    Repo(RepoModeArgs),
    /// Update CGDM for all repos with preloaded_cgdm_blobstore_key configured
    AllRepos(AllReposModeArgs),
}

#[derive(Args)]
pub struct RepoModeArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    /// Blobstore key for the CGDMComponents blob.
    #[clap(long)]
    blobstore_key: String,
}

#[derive(Args, Clone)]
pub struct AllReposModeArgs {
    /// The maximum number of repos to process concurrently.
    #[clap(long, default_value_t = 10)]
    concurrency: usize,
}

#[derive(Clone)]
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
    bookmarks: dyn Bookmarks,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    mutable_repo_blobstore: MutableRepoBlobstore,

    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    config: RepoConfig,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let mut ctx = app.new_basic_context();
    // Force this binary to write to all blobstores
    ctx.session_mut()
        .override_session_class(SessionClass::Background);

    match args.mode {
        Mode::Repo(repo_args) => {
            let repo: Repo = app
                .open_repo(&repo_args.repo)
                .await
                .context("Failed to open repo")?;
            let heads = repo_args
                .changeset_args
                .resolve_changesets(&ctx, &repo)
                .await?;
            update_cgdm(
                &ctx,
                &repo,
                heads,
                repo_args.blobstore_key,
                args.component_max_count,
                args.component_max_size,
                args.rebuild,
            )
            .await
        }
        Mode::AllRepos(all_repos_args) => {
            let app = Arc::new(app);
            update_all_repos(
                &ctx,
                app,
                args.component_max_count,
                args.component_max_size,
                args.rebuild,
                all_repos_args,
            )
            .await
        }
    }
}

async fn update_cgdm(
    ctx: &CoreContext,
    repo: &Repo,
    heads: Vec<mononoke_types::ChangesetId>,
    blobstore_key: String,
    component_max_count: u64,
    component_max_size: u64,
    rebuild: bool,
) -> Result<()> {
    let repo_name = repo.repo_identity().name();
    let mut cgdm_components = match rebuild {
        false => {
            // Try reading from mutable blobstore first, fall back to immutable
            let bytes = match repo
                .mutable_repo_blobstore()
                .get(ctx, &blobstore_key)
                .await?
            {
                Some(bytes) => Some(bytes),
                None => repo.repo_blobstore().get(ctx, &blobstore_key).await?,
            };
            match bytes {
                Some(bytes) => CGDMComponents::from_bytes(bytes.as_raw_bytes())?,
                None => Default::default(),
            }
        }
        true => Default::default(),
    };

    println!(
        "[{}] Loaded {} existing components with {} changesets (rebuild: {})",
        repo_name,
        cgdm_components.components.len(),
        cgdm_components.changeset_to_component_id.len(),
        rebuild,
    );

    let mut component_to_changeset_ids = cgdm_components
        .changeset_to_component_id
        .iter()
        .map(|(cs_id, component_id)| (*component_id, *cs_id))
        .into_group_map();

    // Find all ancestors of heads that are not yet part of CGDM.
    let cs_ids = repo
        .commit_graph()
        .ancestors_difference(ctx, heads, vec![])
        .await?
        .into_iter()
        .rev()
        .filter(|cs_id| {
            !cgdm_components
                .changeset_to_component_id
                .contains_key(cs_id)
        })
        .collect::<Vec<_>>();

    println!(
        "[{}] Found {} new changesets to process",
        repo_name,
        cs_ids.len(),
    );

    // Create a hashmap of all GDMv3 sizes for every changeset
    let gdm_sizes = stream::iter(&cs_ids)
        .map(async |cs_id| {
            repo.repo_derived_data()
                .derive::<RootGitDeltaManifestV3Id>(ctx, *cs_id, DerivationPriority::LOW)
                .await?;
            let gdm = repo
                .repo_derived_data()
                .fetch_derived_direct::<RootGitDeltaManifestV3Id>(ctx, *cs_id)
                .await?
                .ok_or_else(|| anyhow!("No git delta manifest for {cs_id}"))?;

            match gdm {
                GitDeltaManifestV3::Inlined(entries) => {
                    let total_size: u64 = entries
                        .into_iter()
                        .map(|entry| entry.inlined_bytes_size() as u64)
                        .sum();
                    anyhow::Ok((*cs_id, total_size))
                }
                GitDeltaManifestV3::Chunked(_) => Ok((*cs_id, component_max_size + 1)),
            }
        })
        .buffered(1024)
        .try_collect::<HashMap<_, _>>()
        .await?;
    println!("[{repo_name}] Finished calculating GDM sizes");

    // Go through the new commits and for each either create a new component
    // or add them to one of their parent components if possible
    let all_parents = repo
        .commit_graph()
        .many_changeset_parents(ctx, &cs_ids)
        .await?;
    let mut new_full_components = vec![];
    for (index, cs_id) in cs_ids.into_iter().enumerate() {
        let mut found = false;

        let gdm_size = *gdm_sizes
            .get(&cs_id)
            .ok_or_else(|| anyhow!("Can't find GDM size for {cs_id}"))?;

        let parents = all_parents
            .get(&cs_id)
            .ok_or_else(|| anyhow!("Can't find parents for {cs_id}"))?
            .clone();
        let max_parent_component_id = parents
            .iter()
            .map(|parent| {
                *cgdm_components
                    .changeset_to_component_id
                    .get(parent)
                    .expect("Parent should be part of a component")
            })
            .max()
            .unwrap_or(0);

        for parent in parents {
            let parent_component_id = *cgdm_components
                .changeset_to_component_id
                .get(&parent)
                .expect("parent should be part of a component");
            let parent_component_info = cgdm_components
                .components
                .get_mut(&parent_component_id)
                .expect("parent component should exist");

            // We're only allowed to add a commit to a parent component if that component id
            // of that parent is higher than all other parents. This enforces that sorting
            // commits by component id gives a topological order.
            if parent_component_id == max_parent_component_id
                && parent_component_info.changeset_count < component_max_count
                && gdm_size < component_max_size
                && parent_component_info.total_inlined_size < component_max_size
            {
                parent_component_info.changeset_count += 1;
                parent_component_info.total_inlined_size += gdm_size;

                // Invalidate previously set CGDM ids as it indicates
                // use of different component size/count parameters in a previous update.
                parent_component_info.cgdm_id = None;
                parent_component_info.cgdm_commits_id = None;

                cgdm_components
                    .changeset_to_component_id
                    .insert(cs_id, parent_component_id);
                component_to_changeset_ids
                    .entry(parent_component_id)
                    .or_default()
                    .push(cs_id);

                if parent_component_info.changeset_count == component_max_count
                    || parent_component_info.total_inlined_size >= component_max_size
                {
                    new_full_components.push(parent_component_id);
                }

                found = true;
                break;
            }
        }

        if !found {
            let component_id = cgdm_components.components.len() as u64;
            cgdm_components
                .changeset_to_component_id
                .insert(cs_id, component_id);
            component_to_changeset_ids
                .entry(component_id)
                .or_default()
                .push(cs_id);
            cgdm_components.components.insert(
                component_id,
                ComponentInfo {
                    total_inlined_size: gdm_size,
                    changeset_count: 1,
                    cgdm_id: None,
                    cgdm_commits_id: None,
                },
            );
        }

        if (index + 1) % 10000 == 0 {
            println!("[{}] Processed {} changesets", repo_name, index + 1);
        }
    }

    println!(
        "[{}] Finished assigning changesets to {} components",
        repo_name,
        cgdm_components.components.len(),
    );

    println!(
        "[{}] Storing CGDM blobs for {} new full components",
        repo_name,
        new_full_components.len()
    );

    let stored_cgdms = stream::iter(new_full_components)
        .map(async |component_id| {
            let mut changesets = component_to_changeset_ids
                .get(&component_id)
                .expect("component should exist")
                .clone();

            // Sort changesets by generation number (ascending) to ensure topological
            // order in the CGDM blob. Without this, changesets loaded from the HashMap
            // have arbitrary order, causing unresolved deltas during clone.
            let generations = repo
                .commit_graph()
                .many_changeset_generations(ctx, &changesets)
                .await?;
            changesets.sort_by_key(|cs_id| {
                generations
                    .get(cs_id)
                    .copied()
                    .expect("generation should exist for changeset")
            });

            let entries = stream::iter(&changesets)
                .map(async |cs_id| {
                    let gdm = repo
                        .repo_derived_data()
                        .fetch_derived_direct::<RootGitDeltaManifestV3Id>(ctx, *cs_id)
                        .await?
                        .ok_or_else(|| anyhow!("No git delta manifest for {cs_id}"))?;

                    anyhow::Ok(gdm.into_entries(ctx, repo.repo_blobstore()))
                })
                .buffered(1024)
                .try_flatten()
                .try_collect::<Vec<_>>()
                .await?;

            let cgdm = CompactedGitDeltaManifest::new(entries);
            let id = cgdm.into_blob().store(ctx, repo.repo_blobstore()).await?;

            let cgdm_commits = CGDMCommitPackfileItems::new(
                ctx,
                repo.repo_blobstore_arc(),
                repo.bonsai_git_mapping_arc(),
                &changesets,
            )
            .await?;
            let cgdm_commits_id = cgdm_commits
                .into_blob()
                .store(ctx, repo.repo_blobstore())
                .await?;

            anyhow::Ok((component_id, id, cgdm_commits_id))
        })
        .buffered(1024)
        .try_collect::<Vec<_>>()
        .await?;

    for (component_id, id, cgdm_commits_id) in stored_cgdms {
        let component_info = cgdm_components
            .components
            .get_mut(&component_id)
            .expect("component should exist");
        component_info.cgdm_id = Some(id);
        component_info.cgdm_commits_id = Some(cgdm_commits_id);
    }

    println!("[{repo_name}] Saving updated CGDMComponents to blobstore key '{blobstore_key}'",);

    let mapping_bytes = BlobstoreBytes::from_bytes(cgdm_components.into_bytes());
    // Write the CGDMComponents mapping blob to both the immutable and mutable blobstores.
    // The mutable blobstore provides strong consistency guarantees needed for this
    // mutable blob, while the immutable write maintains backwards compatibility.
    futures::try_join!(
        repo.repo_blobstore()
            .put(ctx, blobstore_key.clone(), mapping_bytes.clone()),
        repo.mutable_repo_blobstore()
            .put(ctx, blobstore_key, mapping_bytes),
    )?;

    println!("[{repo_name}] CGDM update complete",);

    Ok(())
}

async fn update_all_repos(
    ctx: &CoreContext,
    app: Arc<MononokeApp>,
    component_max_count: u64,
    component_max_size: u64,
    rebuild: bool,
    all_repos_args: AllReposModeArgs,
) -> Result<()> {
    // Enumerate via `load_all_repo_configs` so split-loaded repos
    // (only present in the per-tier RepoSpec manifest) are included
    // — iterating `repo_configs.repos` would silently skip them, and
    // their CGDM would never get refreshed.
    let applicable_repo_names = app
        .configs()
        .load_all_repo_configs()?
        .into_iter()
        .filter_map(|(name, repo_config)| {
            repo_config
                .git_configs
                .preloaded_cgdm_blobstore_key
                .as_ref()
                .map(|_| name)
        })
        .collect::<Vec<_>>();

    if applicable_repo_names.is_empty() {
        println!("No repos found with preloaded_cgdm_blobstore_key configured");
        return Ok(());
    }

    println!(
        "Found {} repos with preloaded_cgdm_blobstore_key configured",
        applicable_repo_names.len()
    );

    let repo_mgr: MononokeReposManager<Repo> = app
        .open_named_managed_repos(applicable_repo_names, None)
        .await?;
    let repos = Vec::from_iter(repo_mgr.repos().clone().iter());
    stream::iter(repos)
        .map(anyhow::Ok)
        .try_for_each_concurrent(all_repos_args.concurrency, |repo| {
            cloned!(ctx);
            async move {
                let blobstore_key = repo
                    .repo_config()
                    .git_configs
                    .preloaded_cgdm_blobstore_key
                    .clone()
                    .ok_or_else(|| {
                        anyhow!(
                            "Repo {} missing preloaded_cgdm_blobstore_key",
                            repo.repo_identity().name()
                        )
                    })?;
                println!(
                    "Updating CGDM for repo {} with blobstore key {}",
                    repo.repo_identity().name(),
                    &blobstore_key,
                );

                // Resolve all bookmarks as heads
                let heads: Vec<_> = repo
                    .bookmarks()
                    .list(
                        ctx.clone(),
                        Freshness::MostRecent,
                        &BookmarkPrefix::empty(),
                        BookmarkCategory::ALL,
                        BookmarkKind::ALL_PUBLISHING,
                        &BookmarkPagination::FromStart,
                        u64::MAX,
                    )
                    .map_ok(|(_name, cs_id)| cs_id)
                    .try_collect()
                    .await?;

                if heads.is_empty() {
                    println!(
                        "Skipping repo {} - no bookmarks found",
                        repo.repo_identity().name()
                    );
                    return Ok(());
                }

                update_cgdm(
                    &ctx,
                    &repo,
                    heads,
                    blobstore_key,
                    component_max_count,
                    component_max_size,
                    rebuild,
                )
                .await
            }
        })
        .await?;

    Ok(())
}
