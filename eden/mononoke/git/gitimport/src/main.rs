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
use git2::Repository;
use import_tools::{
    git2_oid_to_git_hash_objectid, import_tree_as_single_bonsai_changeset, FullRepoImport,
    GitRangeImport, GitimportPreferences, GitimportTarget, ImportMissingForCommit,
};
use linked_hash_map::LinkedHashMap;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use slog::info;
use std::path::Path;
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
const ARG_DERIVE_TREES: &str = "derive-trees";
const ARG_DERIVE_HG: &str = "derive-hg";
const ARG_HGGIT_COMPATIBILITY: &str = "hggit-compatibility";
const ARG_BONSAI_GIT_MAPPING: &str = "bonsai-git-mapping";
const ARG_SUPPRESS_REF_MAPPING: &str = "suppress-ref-mapping";

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
            Arg::with_name(ARG_DERIVE_TREES)
                .long(ARG_DERIVE_TREES)
                .required(false)
                .takes_value(false),
        )
        .arg(
            Arg::with_name(ARG_DERIVE_HG)
                .long(ARG_DERIVE_HG)
                .required(false)
                .takes_value(false),
        )
        .arg(
            Arg::with_name(ARG_HGGIT_COMPATIBILITY)
                .long(ARG_HGGIT_COMPATIBILITY)
                .help("Set commit extras for hggit compatibility")
                .required(false)
                .takes_value(false),
        )
        .arg(
            Arg::with_name(ARG_BONSAI_GIT_MAPPING)
                .long(ARG_BONSAI_GIT_MAPPING)
                .help("For each created commit also create a bonsai<->git commit mapping.")
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

    if matches.is_present(ARG_DERIVE_TREES) {
        prefs.derive_trees = true;
    }

    if matches.is_present(ARG_DERIVE_HG) {
        prefs.derive_hg = true;
    }

    if matches.is_present(ARG_HGGIT_COMPATIBILITY) {
        prefs.hggit_compatibility = true;
    }

    if matches.is_present(ARG_BONSAI_GIT_MAPPING) {
        prefs.bonsai_git_mapping = true;
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

            let git_repo = Repository::open(&path)?;

            let target: Box<dyn GitimportTarget> = match matches.subcommand() {
                (SUBCOMMAND_FULL_REPO, Some(..)) => Box::new(FullRepoImport {}),
                (SUBCOMMAND_GIT_RANGE, Some(range_matches)) => {
                    let from = range_matches.value_of(ARG_GIT_FROM).unwrap().parse()?;
                    let to = range_matches.value_of(ARG_GIT_TO).unwrap().parse()?;
                    Box::new(GitRangeImport::new(from, to, &ctx, &repo).await?)
                }
                (SUBCOMMAND_MISSING_FOR_COMMIT, Some(matches)) => {
                    let commit = matches.value_of(ARG_GIT_COMMIT).unwrap().parse()?;
                    Box::new(ImportMissingForCommit::new(commit, &ctx, &repo, &git_repo).await?)
                }
                (SUBCOMMAND_IMPORT_TREE_AS_SINGLE_BONSAI_CHANGESET, Some(matches)) => {
                    let commit = matches.value_of(ARG_GIT_COMMIT).unwrap().parse()?;
                    let bcs =
                        import_tree_as_single_bonsai_changeset(&ctx, &repo, path, commit, prefs)
                            .await?;
                    info!(ctx.logger(), "imported as {}", bcs.get_changeset_id());
                    return Ok(());
                }
                _ => {
                    return Err(Error::msg("A valid subcommand is required"));
                }
            };

            let gitimport_result: LinkedHashMap<_, (ChangesetId, BonsaiChangeset)> =
                import_tools::gitimport(&ctx, &repo, &path, &*target, prefs).await?;

            if !matches.is_present(ARG_SUPPRESS_REF_MAPPING) {
                for reference in git_repo.references()? {
                    let reference = reference?;
                    let commit = git2_oid_to_git_hash_objectid(&reference.peel_to_commit()?.id());
                    let bcs_id = gitimport_result.get(&commit).map(|e| e.0);
                    info!(ctx.logger(), "Ref: {:?}: {:?}", reference.name(), bcs_id);
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
