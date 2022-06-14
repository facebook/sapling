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
use bonsai_hg_mapping::{ArcBonsaiHgMapping, MemWritesBonsaiHgMapping};
use cacheblob::{dummy::DummyLease, LeaseOps, MemWritesBlobstore};
use changesets::ArcChangesets;
use clap::{Arg, SubCommand};
use cmdlib::{
    args::{self, RepoRequirement},
    helpers::block_execute,
};
use context::CoreContext;
use fbinit::FacebookInit;
use import_tools::{import_tree_as_single_bonsai_changeset, GitimportPreferences, GitimportTarget};
use linked_hash_map::LinkedHashMap;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use slog::info;
use std::path::{Path, PathBuf};
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

    if matches.is_present(ARG_DERIVE_HG) {
        prefs.derive_hg = true;
    }

    if let Some(path) = matches.value_of(ARG_GIT_COMMAND_PATH) {
        prefs.git_command_path = PathBuf::from(path);
    }

    let path = Path::new(matches.value_of(ARG_GIT_REPOSITORY_PATH).unwrap());

    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

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

            let target = match matches.subcommand() {
                (SUBCOMMAND_FULL_REPO, Some(..)) => GitimportTarget::full(),
                (SUBCOMMAND_GIT_RANGE, Some(range_matches)) => {
                    let from = range_matches.value_of(ARG_GIT_FROM).unwrap().parse()?;
                    let to = range_matches.value_of(ARG_GIT_TO).unwrap().parse()?;
                    GitimportTarget::range(from, to, &ctx, &repo).await?
                }
                (SUBCOMMAND_MISSING_FOR_COMMIT, Some(matches)) => {
                    let commit = matches.value_of(ARG_GIT_COMMIT).unwrap().parse()?;
                    GitimportTarget::missing_for_commit(
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
                    let bcs =
                        import_tree_as_single_bonsai_changeset(&ctx, &repo, path, commit, &prefs)
                            .await?;
                    info!(ctx.logger(), "imported as {}", bcs.get_changeset_id());
                    return Ok(());
                }
                _ => {
                    return Err(Error::msg("A valid subcommand is required"));
                }
            };

            let gitimport_result: LinkedHashMap<_, (ChangesetId, BonsaiChangeset)> =
                import_tools::gitimport(&ctx, &repo, path, &target, &prefs).await?;

            if !matches.is_present(ARG_SUPPRESS_REF_MAPPING) {
                let refs = import_tools::read_git_refs(path, &prefs).await?;
                for (name, commit) in refs {
                    let bcs_id = gitimport_result.get(&commit).map(|e| e.0);
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
