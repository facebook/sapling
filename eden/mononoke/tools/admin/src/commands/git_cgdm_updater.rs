/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::Storable;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMappingArc;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::Bookmarks;
use clap::Parser;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphRef;
use context::SessionClass;
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
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;
use mononoke_types::BlobstoreValue;
use mononoke_types::ThriftConvert;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;

#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    /// The maximum number of changesets in a component.
    #[clap(long)]
    component_max_count: u64,

    /// The maximum size of a component blob in bytes.
    #[clap(long)]
    component_max_size: u64,

    /// Blobstore key for the CGDMComponents blob.
    #[clap(long)]
    blobstore_key: String,

    /// Whether to re-index all commits or start
    /// from the previous blob.
    #[clap(long)]
    rebuild: bool,
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
    repo_identity: RepoIdentity,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let mut ctx = app.new_basic_context();
    // Force this binary to write to all blobstores
    ctx.session_mut()
        .override_session_class(SessionClass::Background);

    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    let mut cgdm_components = match args.rebuild {
        false => match repo.repo_blobstore().get(&ctx, &args.blobstore_key).await? {
            Some(bytes) => CGDMComponents::from_bytes(bytes.as_raw_bytes())?,
            None => Default::default(),
        },
        true => Default::default(),
    };

    let mut component_to_changeset_ids = cgdm_components
        .changeset_to_component_id
        .iter()
        .map(|(cs_id, component_id)| (*component_id, *cs_id))
        .into_group_map();

    // Find all ancestors of heads that are not yet part of CGDM.
    let heads = args.changeset_args.resolve_changesets(&ctx, &repo).await?;
    let cs_ids = repo
        .commit_graph()
        .ancestors_difference(&ctx, heads, vec![])
        .await?
        .into_iter()
        .rev()
        .filter(|cs_id| {
            !cgdm_components
                .changeset_to_component_id
                .contains_key(cs_id)
        })
        .collect::<Vec<_>>();

    // Create a hashmap of all GDMv3 sizes for every changeset
    let gdm_sizes = stream::iter(&cs_ids)
        .map(async |cs_id| {
            repo.repo_derived_data()
                .derive::<RootGitDeltaManifestV3Id>(&ctx, *cs_id)
                .await?;
            let gdm = repo
                .repo_derived_data()
                .fetch_derived_direct::<RootGitDeltaManifestV3Id>(&ctx, *cs_id)
                .await?
                .ok_or_else(|| anyhow!("No git delta manifest for {}", cs_id))?;

            match gdm {
                GitDeltaManifestV3::Inlined(entries) => {
                    let total_size: u64 = entries
                        .into_iter()
                        .map(|entry| entry.inlined_bytes_size() as u64)
                        .sum();
                    anyhow::Ok((*cs_id, total_size))
                }
                GitDeltaManifestV3::Chunked(_) => Ok((*cs_id, args.component_max_size + 1)),
            }
        })
        .buffered(1024)
        .try_collect::<HashMap<_, _>>()
        .await?;
    println!("Finished calculating gdm sizes");

    // Go through the new commits and for each either create a new component
    // or add them to one of their parent components if possible
    let mut new_full_components = vec![];
    for (index, cs_id) in cs_ids.into_iter().enumerate() {
        let mut found = false;

        let gdm_size = *gdm_sizes
            .get(&cs_id)
            .ok_or_else(|| anyhow!("Can't find GDM size for {}", cs_id))?;

        let parents = repo.commit_graph().changeset_parents(&ctx, cs_id).await?;
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
                && parent_component_info.changeset_count < args.component_max_count
                && gdm_size < args.component_max_size
                && parent_component_info.total_inlined_size < args.component_max_size
            {
                parent_component_info.changeset_count += 1;
                parent_component_info.total_inlined_size += gdm_size;
                cgdm_components
                    .changeset_to_component_id
                    .insert(cs_id, parent_component_id);
                component_to_changeset_ids
                    .entry(parent_component_id)
                    .or_default()
                    .push(cs_id);

                if parent_component_info.changeset_count == args.component_max_count
                    || parent_component_info.total_inlined_size >= args.component_max_size
                {
                    new_full_components.push(parent_component_id);
                }

                found = true;
                break;
            }
        }

        if !found {
            let component_id = cgdm_components.changeset_to_component_id.len() as u64;
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
            println!("Processed {} changesets", index + 1);
        }
    }

    println!(
        "Storing CGDM blobs for {} new full components",
        new_full_components.len()
    );

    let stored_cgdms = stream::iter(new_full_components)
        .map(async |component_id| {
            let changesets = component_to_changeset_ids
                .get(&component_id)
                .expect("component should exist");
            let entries = stream::iter(changesets)
                .map(async |cs_id| {
                    let gdm = repo
                        .repo_derived_data()
                        .fetch_derived_direct::<RootGitDeltaManifestV3Id>(&ctx, *cs_id)
                        .await?
                        .ok_or_else(|| anyhow!("No git delta manifest for {}", cs_id))?;

                    anyhow::Ok(gdm.into_entries(&ctx, repo.repo_blobstore()))
                })
                .buffered(1024)
                .try_flatten()
                .try_collect::<Vec<_>>()
                .await?;

            let cgdm = CompactedGitDeltaManifest::new(entries);
            let id = cgdm.into_blob().store(&ctx, repo.repo_blobstore()).await?;

            let cgdm_commits = CGDMCommitPackfileItems::new(
                &ctx,
                repo.repo_blobstore_arc(),
                repo.bonsai_git_mapping_arc(),
                changesets,
            )
            .await?;
            let cgdm_commits_id = cgdm_commits
                .into_blob()
                .store(&ctx, repo.repo_blobstore())
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

    repo.repo_blobstore()
        .put(
            &ctx,
            args.blobstore_key,
            BlobstoreBytes::from_bytes(cgdm_components.into_bytes()),
        )
        .await?;

    Ok(())
}
