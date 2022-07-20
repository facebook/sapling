/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod mem_writes_changesets;

use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use blobstore::Blobstore;
use blobstore::Loadable;
use bonsai_hg_mapping::ArcBonsaiHgMapping;
use bonsai_hg_mapping::MemWritesBonsaiHgMapping;
use cacheblob::dummy::DummyLease;
use cacheblob::LeaseOps;
use cacheblob::MemWritesBlobstore;
use changesets::ArcChangesets;
use clap::Arg;
use clap::SubCommand;
use cmdlib::args;
use cmdlib::args::RepoRequirement;
use cmdlib::helpers::block_execute;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future;
use import_tools::import_tree_as_single_bonsai_changeset;
use import_tools::GitimportPreferences;
use import_tools::GitimportTarget;
use linked_hash_map::LinkedHashMap;
use mercurial_derived_data::get_manifest_from_bonsai;
use mercurial_derived_data::DeriveHgChangeset;
use mononoke_types::ChangesetId;
use slog::info;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::mem_writes_changesets::MemWritesChangesets;

// Refactor this a bit. Use a thread pool for git operations. Pass that wherever we use store repo.
// Transform the walk into a stream of commit + file changes.

const SUBCOMMAND_FULL_REPO: &str = "full-repo";
const SUBCOMMAND_GIT_RANGE: &str = "git-range";
const SUBCOMMAND_MISSING_FOR_COMMIT: &str = "missing-for-commit";
const SUBCOMMAND_IMPORT_TREE_AS_SINGLE_BONSAI_CHANGESET: &str =
    "import-tree-as-single-bonsai-changeset";

const ARG_GIT_REPOSITORY_PATH: &str = "git-repository-path";
const ARG_DERIVE_HG: &str = "derive-hg";
const ARG_SUPPRESS_REF_MAPPING: &str = "suppress-ref-mapping";
const ARG_GIT_COMMAND_PATH: &str = "git-command-path";

const ARG_GIT_FROM: &str = "git-from";
const ARG_GIT_TO: &str = "git-to";

const ARG_GIT_COMMIT: &str = "git-commit";

const ARG_REUPLOAD_COMMITS: &str = "reupload-commits";

async fn derive_hg(
    ctx: &CoreContext,
    repo: &BlobRepo,
    import_map: impl Iterator<Item = (&git_hash::ObjectId, &ChangesetId)>,
) -> Result<(), Error> {
    let mut hg_manifests = HashMap::new();

    for (id, bcs_id) in import_map {
        let bcs = bcs_id.load(ctx, repo.blobstore()).await?;
        let parent_manifests = future::try_join_all(bcs.parents().map({
            let hg_manifests = &hg_manifests;
            move |p| async move {
                let manifest = if let Some(manifest) = hg_manifests.get(&p) {
                    *manifest
                } else {
                    repo.derive_hg_changeset(ctx, p)
                        .await?
                        .load(ctx, repo.blobstore())
                        .await?
                        .manifestid()
                };
                Result::<_, Error>::Ok(manifest)
            }
        }))
        .await?;

        let manifest = get_manifest_from_bonsai(
            ctx.clone(),
            repo.get_blobstore().boxed(),
            bcs.clone(),
            parent_manifests,
        )
        .await?;

        hg_manifests.insert(*bcs_id, manifest);

        info!(ctx.logger(), "Hg: {:?}: {:?}", id, manifest);
    }

    Ok(())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Mononoke Git Importer")
        .with_repo_required(RepoRequirement::ExactlyOne)
        .with_fb303_args()
        .build()
        .arg(
            Arg::with_name(ARG_DERIVE_HG)
                .long(ARG_DERIVE_HG)
                .required(false)
                .takes_value(false),
        )
        .arg(
            Arg::with_name(ARG_SUPPRESS_REF_MAPPING)
                .long(ARG_SUPPRESS_REF_MAPPING)
                .help("This is used to suppress the printing of the potentially really long git Reference -> BonzaiID mapping.")
                .required(false)
                .takes_value(false),
        )
        .arg(
            Arg::with_name(ARG_GIT_COMMAND_PATH)
                .long(ARG_GIT_COMMAND_PATH)
                .help("Set the path to the git binary - preset to git.real")
                .required(false)
                .takes_value(true),
        )
        .arg(Arg::with_name(ARG_GIT_REPOSITORY_PATH).help("Path to a git repository to import"))
        .arg(
            Arg::with_name(ARG_REUPLOAD_COMMITS)
                .long(ARG_REUPLOAD_COMMITS)
                .help("Reupload git commits, even if they already exist in Mononoke")
                .required(false)
                .takes_value(false)
        )
        .subcommand(SubCommand::with_name(SUBCOMMAND_FULL_REPO))
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_GIT_RANGE)
                .arg(
                    Arg::with_name(ARG_GIT_FROM)
                        .required(true)
                        .takes_value(true),
                )
                .arg(Arg::with_name(ARG_GIT_TO).required(true).takes_value(true)),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_MISSING_FOR_COMMIT).arg(
                Arg::with_name(ARG_GIT_COMMIT)
                    .required(true)
                    .takes_value(true),
            ),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_IMPORT_TREE_AS_SINGLE_BONSAI_CHANGESET).arg(
                Arg::with_name(ARG_GIT_COMMIT)
                    .required(true)
                    .takes_value(true),
            ),
        );

    let mut prefs = GitimportPreferences::default();

    let matches = app.get_matches(fb)?;

    // if we are readonly, then we'll set up some overrides to still be able to do meaningful
    // things below.
    let dry_run = matches.readonly_storage().0;
    prefs.dry_run = dry_run;

    let derive_hg_data = matches.is_present(ARG_DERIVE_HG);

    if let Some(path) = matches.value_of(ARG_GIT_COMMAND_PATH) {
        prefs.git_command_path = PathBuf::from(path);
    }

    let path = Path::new(matches.value_of(ARG_GIT_REPOSITORY_PATH).unwrap());

    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let reupload = if matches.is_present(ARG_REUPLOAD_COMMITS) {
        import_direct::ReuploadCommits::Always
    } else {
        import_direct::ReuploadCommits::Never
    };

    let repo = args::create_repo(fb, logger, &matches);
    block_execute(
        async {
            let repo: BlobRepo = repo.await?;

            let repo = if dry_run {
                repo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
                    Arc::new(MemWritesBlobstore::new(blobstore))
                })
                .dangerous_override(|changesets| -> ArcChangesets {
                    Arc::new(MemWritesChangesets::new(changesets))
                })
                .dangerous_override(|bonsai_hg_mapping| -> ArcBonsaiHgMapping {
                    Arc::new(MemWritesBonsaiHgMapping::new(bonsai_hg_mapping))
                })
                .dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>)
            } else {
                repo
            };

            let uploader = import_direct::DirectUploader::new(repo.clone(), reupload);

            let target = match matches.subcommand() {
                (SUBCOMMAND_FULL_REPO, Some(..)) => GitimportTarget::full(),
                (SUBCOMMAND_GIT_RANGE, Some(range_matches)) => {
                    let from = range_matches.value_of(ARG_GIT_FROM).unwrap().parse()?;
                    let to = range_matches.value_of(ARG_GIT_TO).unwrap().parse()?;
                    import_direct::range(from, to, &ctx, &repo).await?
                }
                (SUBCOMMAND_MISSING_FOR_COMMIT, Some(matches)) => {
                    let commit = matches.value_of(ARG_GIT_COMMIT).unwrap().parse()?;
                    import_direct::missing_for_commit(
                        commit,
                        &ctx,
                        &repo,
                        &prefs.git_command_path,
                        path,
                    )
                    .await?
                }
                (SUBCOMMAND_IMPORT_TREE_AS_SINGLE_BONSAI_CHANGESET, Some(matches)) => {
                    let commit = matches.value_of(ARG_GIT_COMMIT).unwrap().parse()?;
                    let bcs_id = import_tree_as_single_bonsai_changeset(
                        &ctx, path, uploader, commit, &prefs,
                    )
                    .await?;
                    info!(ctx.logger(), "imported as {}", bcs_id);
                    if derive_hg_data {
                        derive_hg(&ctx, &repo, [(&commit, &bcs_id)].into_iter()).await?;
                    }
                    return Ok(());
                }
                _ => {
                    return Err(Error::msg("A valid subcommand is required"));
                }
            };

            let gitimport_result: LinkedHashMap<_, _> =
                import_tools::gitimport(&ctx, path, uploader, &target, &prefs).await?;
            if derive_hg_data {
                derive_hg(&ctx, &repo, gitimport_result.iter()).await?;
            }

            if !matches.is_present(ARG_SUPPRESS_REF_MAPPING) {
                let refs = import_tools::read_git_refs(path, &prefs).await?;
                for (name, commit) in refs {
                    let bcs_id = gitimport_result.get(&commit);
                    info!(
                        ctx.logger(),
                        "Ref: {:?}: {:?}",
                        String::from_utf8_lossy(&name),
                        bcs_id
                    );
                }
            }

            Ok(())
        },
        fb,
        "gitimport",
        logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
