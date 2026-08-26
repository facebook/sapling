/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use bookmarks_movement::BookmarkKindRestrictions;
use bookmarks_movement::CrossRepoPushSource;
use bytes::Bytes;
use changesets_creation::save_changesets;
use clap::Parser;
use clap::ValueEnum;
use commit_graph::CommitGraphArc;
use commit_graph::LinearAncestorsStreamBuilder;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use hook_manager::manager::HookManagerRef;
use manifest::ManifestOps;
use manifest::PathOrPrefix;
#[cfg(fbcode_build)]
use metaconfig_types::Address;
#[cfg(fbcode_build)]
use metaconfig_types::PushrebaseRemoteMode;
#[cfg(fbcode_build)]
use metaconfig_types::RepoConfigRef;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::GitLfs;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
// land_service is FB-internal only; keep it out of OSS builds.
#[cfg(fbcode_build)]
use pushrebase_client::LandServicePushrebaseClient;
use pushrebase_client::LocalPushrebaseClient;
use pushrebase_client::PushrebaseClient;
use repo_authorization::AuthorizationContext;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use restricted_paths::RestrictedPathsRef;
use sha2::Digest;
use sha2::Sha256;
use skeleton_manifest::RootSkeletonManifestId;

use super::Repo;

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum LandBackend {
    LandService,
    Local,
}

/// Land many file-disjoint stacks in parallel onto a single test bookmark and measure commit throughput
#[derive(Parser)]
pub struct CommitThroughputArgs {
    /// Number of stacks to land.
    #[clap(long, default_value_t = 20)]
    stacks: usize,

    /// Commits per stack.
    #[clap(long, default_value_t = 1)]
    stack_size: usize,

    /// Master commits to scan while assembling the requested shape.
    #[clap(long, default_value_t = 1000)]
    max_scan: usize,

    /// Target bookmark.
    #[clap(long, default_value = "mononoke_commit_throughput_test")]
    bookmark: String,

    /// Bookmark whose history is replayed.
    #[clap(long, default_value = "master")]
    source_bookmark: String,

    /// Commit author.
    #[clap(
        long,
        default_value = "SCM Commit Throughput Bot <generatedunixname1704614544113542@meta.com>"
    )]
    author: String,

    /// Lands to keep in flight (0 = submit every stack at once).
    #[clap(long, default_value_t = 0)]
    concurrency: usize,

    /// Where the land happens: the prod land_service, or in-process pushrebase.
    #[clap(long, value_enum, default_value_t = LandBackend::LandService)]
    land_backend: LandBackend,

    /// Land against this land_service host:port instead of the address in repo
    /// config. Use to point at a locally running land_service.
    #[clap(long)]
    land_service_host_port: Option<String>,

    /// Land as this service identity. Unset lands as the calling user.
    #[clap(long)]
    service_identity: Option<String>,

    /// Select and group commits, then stop without writing anything.
    #[clap(long)]
    dry_run: bool,
}

struct LandedStack {
    commits: usize,
    submitted_at_secs: f64,
    landed_at_secs: f64,
    pushrebase_distance: u64,
    pushrebase_retries: u64,
}

pub async fn commit_throughput(
    ctx: &CoreContext,
    repo: &Repo,
    args: CommitThroughputArgs,
) -> Result<()> {
    let bookmark = BookmarkKey::new(&args.bookmark)?;
    if args.bookmark == args.source_bookmark {
        bail!("refusing to land onto {bookmark}, the bookmark being replayed");
    }

    let head = repo
        .bookmarks()
        .get(
            ctx.clone(),
            &BookmarkKey::new(&args.source_bookmark)?,
            Freshness::MostRecent,
        )
        .await?
        .with_context(|| format!("bookmark '{}' not found", args.source_bookmark))?;
    let mut walked: Vec<_> =
        LinearAncestorsStreamBuilder::new(repo.commit_graph_arc(), ctx.clone(), head)
            .await?
            .build()
            .await?
            .take(args.max_scan + 1)
            .try_collect()
            .await?;
    let base = walked.pop().context("no master commits found")?;
    if walked.is_empty() {
        bail!(
            "{} has no commits to replay above the base",
            args.source_bookmark
        );
    }
    let nodes: Vec<_> = walked.into_iter().rev().collect();

    let changes: HashMap<_, (Vec<_>, Vec<_>, DateTime)> = stream::iter(nodes.iter().copied())
        .map(|cs_id| async move {
            let bonsai = cs_id.load(ctx, repo.repo_blobstore()).await?;
            let mut changed = Vec::new();
            let mut files = Vec::new();
            let mut deletions = Vec::new();
            for (path, change) in bonsai.file_changes() {
                changed.push(path.clone());
                let Some(basic) = change.simplify() else {
                    deletions.push(path.clone());
                    continue;
                };
                if basic.size() <= 5 * 1024 * 1024 - 4096 {
                    files.push((
                        path.clone(),
                        basic.content_id(),
                        basic.file_type(),
                        basic.size(),
                    ));
                }
            }
            let restricted = !repo
                .restricted_paths()
                .get_path_restriction_info(ctx, Some(cs_id), &changed)
                .await?
                .is_empty();
            anyhow::Ok((!restricted).then_some((cs_id, (files, deletions, *bonsai.author_date()))))
        })
        .buffer_unordered(64)
        .try_filter_map(|replayable| async move { Ok(replayable) })
        .try_collect()
        .await?;

    let mut owner: HashMap<NonRootMPath, usize> = HashMap::new();
    let mut stacks: Vec<Vec<_>> = Vec::new();
    for cs_id in &nodes {
        let Some((files, deletions, _)) = changes.get(cs_id) else {
            continue;
        };
        if files.is_empty() {
            continue;
        }
        let paths: Vec<NonRootMPath> = files
            .iter()
            .map(|(path, ..)| path.clone())
            .chain(deletions.iter().cloned())
            .collect();

        let overlapping: HashSet<usize> = paths
            .iter()
            .filter_map(|path| owner.get(path).copied())
            .collect();
        let mut overlapping = overlapping.into_iter();
        let target = match (overlapping.next(), overlapping.next()) {
            (None, _) => {
                stacks.push(Vec::new());
                Some(stacks.len() - 1)
            }
            (Some(stack), None) => Some(stack),
            _ => None,
        };
        let Some(target) = target else {
            continue;
        };

        stacks[target].push(*cs_id);
        for path in paths {
            owner.insert(path, target);
        }
    }

    let mut shaped: Vec<Vec<_>> = Vec::new();
    let mut current: Vec<_> = Vec::new();
    for group in stacks {
        if shaped.len() == args.stacks {
            break;
        }
        current.extend(group.into_iter().take(args.stack_size - current.len()));
        if current.len() == args.stack_size {
            shaped.push(std::mem::take(&mut current));
        }
    }
    if shaped.len() < args.stacks {
        bail!(
            "could only assemble {}/{} stacks of {} commit(s) from {} scanned commits; \
             raise --max-scan or lower --stacks/--stack-size",
            shaped.len(),
            args.stacks,
            args.stack_size,
            nodes.len()
        );
    }

    let mut stage_mix: BTreeMap<String, usize> = BTreeMap::new();
    for cs_id in shaped.iter().flatten() {
        let tops: HashSet<String> = changes[cs_id]
            .0
            .iter()
            .map(|(path, ..)| path)
            .chain(changes[cs_id].1.iter())
            .map(|path| match path.num_components() {
                1 => String::from("(root)"),
                _ => path
                    .iter()
                    .next()
                    .expect("non-root path has a first element")
                    .to_string(),
            })
            .collect();
        for top in tops {
            *stage_mix.entry(top).or_default() += 1;
        }
    }
    println!(
        "Assembled {} stacks x {} commit(s) = {} commits from {} scanned.",
        shaped.len(),
        args.stack_size,
        shaped.len() * args.stack_size,
        nodes.len()
    );
    println!(
        "  stage mix (commits touching each top-level tree): {}",
        stage_mix
            .iter()
            .map(|(stage, count)| format!("{stage}={count}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    if args.dry_run {
        println!("[dry-run] wrote nothing.");
        return Ok(());
    }

    let base_skeleton = repo
        .repo_derived_data()
        .derive::<RootSkeletonManifestId>(ctx, base, DerivationPriority::LOW)
        .await?
        .into_skeleton_manifest_id();
    let present_at_base: HashSet<MPath> = base_skeleton
        .find_entries(
            ctx.clone(),
            repo.repo_blobstore().clone(),
            shaped
                .iter()
                .flatten()
                .flat_map(|cs_id| changes[cs_id].1.iter())
                .map(|path| PathOrPrefix::Path(path.clone().into()))
                .collect::<Vec<_>>(),
        )
        .map_ok(|(path, _)| path)
        .try_collect()
        .await?;

    let mut built: Vec<Vec<_>> = Vec::new();
    let mut all = Vec::new();
    for stack in &shaped {
        let mut parents = vec![base];
        let mut changesets = Vec::new();
        let mut live: HashSet<NonRootMPath> = stack
            .iter()
            .flat_map(|cs_id| changes[cs_id].1.iter())
            .filter(|path| present_at_base.contains(&MPath::from((*path).clone())))
            .cloned()
            .collect();
        for cs_id in stack {
            let (files, deletions, author_date) = &changes[cs_id];
            let mut file_changes: Vec<_> = stream::iter(files.iter())
                .map(|(path, content_id, file_type, size)| async move {
                    let data = filestore::fetch_concat_exact(
                        repo.repo_blobstore(),
                        ctx,
                        *content_id,
                        *size,
                    )
                    .await?;
                    let digest = hex::encode(Sha256::digest(&data));
                    let nonced = Bytes::from(
                        [&data[..], format!("\n# drill-nonce {digest}\n").as_bytes()].concat(),
                    );
                    let len = nonced.len() as u64;
                    let stored = filestore::store(
                        repo.repo_blobstore(),
                        *repo.filestore_config(),
                        ctx,
                        &StoreRequest::new(len),
                        stream::once(async move { Ok(nonced) }),
                    )
                    .await?;
                    anyhow::Ok((
                        path.clone(),
                        FileChange::tracked(
                            stored.content_id,
                            *file_type,
                            len,
                            None,
                            GitLfs::FullContent,
                        ),
                    ))
                })
                .buffer_unordered(64)
                .try_collect()
                .await?;
            for (path, _) in &file_changes {
                live.insert(path.clone());
            }
            for path in deletions {
                if live.remove(path) {
                    file_changes.push((path.clone(), FileChange::Deletion));
                }
            }

            let changeset = BonsaiChangesetMut {
                parents: parents.clone(),
                author: args.author.clone(),
                author_date: *author_date,
                message: format!("[drill] synthetic replay of {cs_id}"),
                file_changes: file_changes.into_iter().collect(),
                ..Default::default()
            }
            .freeze()?;
            parents = vec![changeset.get_changeset_id()];
            changesets.push(changeset);
        }
        all.extend(changesets.iter().cloned());
        built.push(changesets);
    }
    if built.len() != args.stacks || built.iter().any(|stack| stack.len() != args.stack_size) {
        bail!(
            "built {} stacks with sizes {:?}, expected exactly {} x {}",
            built.len(),
            built.iter().map(Vec::len).collect::<Vec<_>>(),
            args.stacks,
            args.stack_size
        );
    }
    let commits = all.len();
    save_changesets(ctx, repo, all).await?;
    println!(
        "Built {} stacks / {commits} commits on base {}.",
        built.len(),
        base
    );

    let mut txn = repo.bookmarks().create_transaction(ctx.clone());
    txn.force_set(&bookmark, base, BookmarkUpdateReason::ManualMove)?;
    txn.commit().await?;
    println!("Reset {bookmark} to {base}.");

    let authz = match &args.service_identity {
        Some(service_identity) => AuthorizationContext::new_for_service_writes(service_identity),
        None => AuthorizationContext::new(ctx),
    };
    let client: Box<dyn PushrebaseClient + '_> = match args.land_backend {
        #[cfg(fbcode_build)]
        LandBackend::LandService => match &args.land_service_host_port {
            Some(host_port) => Box::new(
                LandServicePushrebaseClient::from_host_port(ctx, host_port.clone(), &authz, repo)
                    .await?,
            ),
            None => {
                let (PushrebaseRemoteMode::RemoteLandService(address)
                | PushrebaseRemoteMode::RemoteLandServiceWithLocalFallback(address)) =
                    &repo.repo_config().pushrebase.remote_mode
                else {
                    bail!("repo is not configured for a remote land_service");
                };
                match address {
                    Address::Tier(tier) => Box::new(
                        LandServicePushrebaseClient::from_tier(ctx, tier.clone(), &authz, repo)
                            .await?,
                    ),
                    Address::HostPort(host_port) => Box::new(
                        LandServicePushrebaseClient::from_host_port(
                            ctx,
                            host_port.clone(),
                            &authz,
                            repo,
                        )
                        .await?,
                    ),
                }
            }
        },
        #[cfg(not(fbcode_build))]
        LandBackend::LandService => {
            bail!("the land_service backend is only available in fbcode builds")
        }
        LandBackend::Local => Box::new(LocalPushrebaseClient {
            ctx,
            authz: &authz,
            repo,
            hook_manager: repo.hook_manager(),
        }),
    };

    println!("Landing {} stacks in parallel...", built.len());
    let wall = Instant::now();
    let in_flight = match args.concurrency {
        0 => built.len(),
        limit => limit,
    };
    let results: Vec<Result<LandedStack>> = stream::iter(built.iter().map(|changesets| {
        let client = &client;
        let bookmark = &bookmark;
        async move {
            let submitted_at_secs = wall.elapsed().as_secs_f64();
            let outcome = client
                .pushrebase(
                    bookmark,
                    changesets,
                    None,
                    CrossRepoPushSource::NativeToThisRepo,
                    BookmarkKindRestrictions::AnyKind,
                    true,
                )
                .await?;
            let landed_at_secs = wall.elapsed().as_secs_f64();
            println!(
                "  landed {} ({} commit(s)) at +{landed_at_secs:.3}s: distance {}, retries {}, took {:.3}s",
                outcome.head,
                changesets.len(),
                outcome.pushrebase_distance.0,
                outcome.retry_num.0,
                landed_at_secs - submitted_at_secs,
            );
            anyhow::Ok(LandedStack {
                commits: changesets.len(),
                submitted_at_secs,
                landed_at_secs,
                pushrebase_distance: outcome.pushrebase_distance.0 as u64,
                pushrebase_retries: outcome.retry_num.0 as u64,
            })
        }
    }))
    .buffer_unordered(in_flight)
    .collect()
    .await;
    let wall = wall.elapsed().as_secs_f64();

    let landed: Vec<&LandedStack> = results
        .iter()
        .filter_map(|result| result.as_ref().ok())
        .collect();
    let commits: usize = landed.iter().map(|land| land.commits).sum();
    println!(
        "\nParallel done: {}/{} stacks landed, {commits} commits over {wall:.1}s wall \
         ({:.2} commits/sec).",
        landed.len(),
        built.len(),
        commits as f64 / wall,
    );
    for error in results.iter().filter_map(|result| result.as_ref().err()) {
        println!("  stack land failed: {error:?}");
    }
    if landed.is_empty() {
        return Ok(());
    }

    let mut submits: Vec<f64> = landed.iter().map(|land| land.submitted_at_secs).collect();
    let mut dones: Vec<f64> = landed.iter().map(|land| land.landed_at_secs).collect();
    submits.sort_by(f64::total_cmp);
    dones.sort_by(f64::total_cmp);
    println!(
        "Submit window: all {} lands submitted between {:.3}s and {:.3}s (spread {:.3}s); \
         responses returned between {:.3}s and {:.3}s.",
        landed.len(),
        submits[0],
        submits[submits.len() - 1],
        submits[submits.len() - 1] - submits[0],
        dones[0],
        dones[dones.len() - 1],
    );

    let mut latencies: Vec<f64> = landed
        .iter()
        .map(|land| land.landed_at_secs - land.submitted_at_secs)
        .collect();
    latencies.sort_by(f64::total_cmp);
    let last = latencies.len() - 1;
    println!(
        "Per-stack land latency: avg {:.3}s, p50 {:.3}s, p90 {:.3}s, max {:.3}s.",
        latencies.iter().sum::<f64>() / latencies.len() as f64,
        latencies[last / 2],
        latencies[(last as f64 * 0.9).round() as usize],
        latencies[last],
    );

    let mut distances: Vec<u64> = landed.iter().map(|land| land.pushrebase_distance).collect();
    distances.sort_unstable();
    println!(
        "Server pushrebase distance: p50 {}, max {}.",
        distances[distances.len() / 2],
        distances[distances.len() - 1],
    );

    let mut retries: Vec<u64> = landed.iter().map(|land| land.pushrebase_retries).collect();
    let retried = retries.iter().filter(|retry| **retry > 0).count();
    retries.sort_unstable();
    println!(
        "Retry count: p50 {}, max {}, {retried} stacks retried.",
        retries[retries.len() / 2],
        retries[retries.len() - 1],
    );
    Ok(())
}
