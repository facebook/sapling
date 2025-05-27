/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroUsize;
use std::time::Duration;

use clap::Parser;
use clap::Subcommand;
use mononoke_app::args::RepoArgs;
use mononoke_types::DateTime;

use crate::ImportStage;
use crate::RecoveryFields;

#[derive(Parser)]
#[clap(about = "Check for additional setup steps before running the repo_import tool")]
pub struct CheckAdditionalSetupStepsArgs {
    /// Disable waiting for Phabricator to parse commits.
    #[clap(long)]
    pub disable_phabricator_check: bool,
    /// Suffix of the bookmark (repo_import_<suffix>). \
    //  This bookmark is used to publish the imported commits and to track the parsing of commits on Phabricator.
    #[clap(long)]
    pub bookmark_suffix: String,
    ///The bookmark branch we want to merge our repo into (e.g. master)
    #[clap(long)]
    pub dest_bookmark: String,
}

//import
#[derive(Parser)]
#[clap(about = "Run the whole repo_import process")]
pub struct ImportArgs {
    /// File path to fetch the recovery state for repo_import tool.
    pub git_repository_path: String,
    /// Revision in a git repo which should be merged
    #[clap(long)]
    pub git_merge_rev_id: String,
    ///Path to the destination folder we import to
    #[clap(long)]
    pub dest_path: String,
    ///Number of commits we make visible when moving the bookmark
    #[clap(long, default_value_t = NonZeroUsize::new(100).unwrap())]
    pub batch_size: NonZeroUsize,
    #[clap(flatten)]
    pub additional_setup_steps_args: CheckAdditionalSetupStepsArgs,
    ///Disable x_repo sync check after moving the bookmark
    #[clap(long)]
    pub disable_x_repo_check: bool,
    ///Sleep time in seconds, if we fail dependent system (phabricator, etc.) checkers
    #[clap(long, default_value_t = 5)]
    pub sleep_time_secs: u64,
    ///commit author to use
    #[clap(long)]
    pub commit_author: String,
    ///commit message to use
    #[clap(long)]
    pub commit_message: String,
    ///commit date to use (default is now)
    #[clap(long, value_parser = DateTime::from_rfc3339)]
    pub commit_date_rfc3339: Option<DateTime>,
    ///File path to store the importing state for recovery in case the tool breaks
    #[clap(long)]
    pub recovery_file_path: String,
    /// If this is an import into a large repo, then it must not be to a
    /// location that is mapped to a small repo (if it is, import to the small
    /// repo instead) and we should mark commits as not synced using a mapping
    /// which does not map to any small repo.  This argument is used to
    /// specify the name of that mapping.
    #[clap(long)]
    pub mark_not_synced_mapping: Option<String>,
    /// Print the mapping of git commit id -> bonsai changeset id after importing
    /// all git commits.
    #[clap(long, default_value_t = false)]
    pub print_gitimport_map: bool,
}

//recover-process
#[derive(Parser)]
#[clap(about = "Repo_import tool process recovery in case of import failure")]
pub struct RecoverProcessArgs {
    /// Path to a git repository to import
    pub saved_recovery_file_path: String,
}

#[derive(Subcommand)]
pub enum Commands {
    CheckAdditionalSetupSteps(CheckAdditionalSetupStepsArgs),
    Import(ImportArgs),
    RecoverProcess(RecoverProcessArgs),
}

#[derive(Parser)]
#[clap(about = "Automating repository imports")]
pub struct MononokeRepoImportArgs {
    /// The repository name or ID
    #[clap(flatten)]
    pub repo: RepoArgs,
    /// Error out before the actual merge step. Useful for running all the a
    /// earlier steps to prepare for merge time.
    #[clap(long)]
    pub no_merge: bool,
    /// Set the path to the git binary - preset to git.real
    #[clap(long)]
    pub git_command_path: Option<String>,
    #[clap(subcommand)]
    pub command: Option<Commands>,
}

pub fn setup_import_args(import_args: ImportArgs) -> RecoveryFields {
    RecoveryFields {
        import_stage: ImportStage::GitImport,
        recovery_file_path: import_args.recovery_file_path,
        git_merge_rev_id: import_args.git_merge_rev_id,
        git_repo_path: import_args.git_repository_path,
        dest_path: import_args.dest_path,
        bookmark_suffix: import_args.additional_setup_steps_args.bookmark_suffix,
        batch_size: import_args.batch_size.get(),
        move_bookmark_commits_done: 0,
        phab_check_disabled: import_args
            .additional_setup_steps_args
            .disable_phabricator_check,
        x_repo_check_disabled: import_args.disable_x_repo_check,
        sleep_time: Duration::from_secs(import_args.sleep_time_secs),
        dest_bookmark_name: import_args.additional_setup_steps_args.dest_bookmark,
        commit_author: import_args.commit_author,
        commit_message: import_args.commit_message,
        datetime: import_args
            .commit_date_rfc3339
            .unwrap_or_else(DateTime::now),
        mark_not_synced_mapping: import_args.mark_not_synced_mapping,
        imported_cs_id: None,
        shifted_bcs_ids: None,
        gitimport_bcs_ids: None,
        merged_cs_id: None,
        git_merge_bcs_id: None,
        print_gitimport_map: import_args.print_gitimport_map,
    }
}
