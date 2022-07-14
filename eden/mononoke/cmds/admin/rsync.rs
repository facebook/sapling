/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use copy_utils::copy;
use copy_utils::remove_excessive_files;
use copy_utils::Limits;
use copy_utils::Options;
use fbinit::FacebookInit;
use futures::future::try_join;

use blobrepo::BlobRepo;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use regex::Regex;
use slog::warn;
use slog::Logger;
use std::num::NonZeroU64;

use crate::common::get_source_target_repos_and_mapping;
use crate::error::SubcommandError;

pub const ARG_COMMIT_AUTHOR: &str = "commit-author";
pub const ARG_COMMIT_MESSAGE: &str = "commit-message";
pub const ARG_SOURCE_CSID: &str = "source-csid";
pub const ARG_TARGET_CSID: &str = "target-csid";
pub const ARG_EXCLUDE_FILE_REGEX: &str = "exclude-file-regex";
pub const ARG_TOTAL_FILE_NUM_LIMIT: &str = "total-file-num-limit";
pub const ARG_TOTAL_SIZE_LIMIT: &str = "total-size-limit";
pub const ARG_FROM_DIR: &str = "from-dir";
pub const ARG_FROM_TO_DIRS: &str = "from-to-dirs";
pub const ARG_LFS_THRESHOLD: &str = "lfs-threshold";
pub const ARG_OVERWRITE: &str = "overwrite";
pub const ARG_TO_DIR: &str = "to-dir";
pub const RSYNC: &str = "rsync";
pub const SUBCOMMAND_COPY: &str = "copy";
pub const SUBCOMMAND_REMOVE_EXCESSIVE_FILES: &str = "remove-excessive-files";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(RSYNC)
        .subcommand(
            add_common_args(SubCommand::with_name(SUBCOMMAND_COPY)
            .about("creates commits that copy content of one directory into another")
            .arg(
                Arg::with_name(ARG_EXCLUDE_FILE_REGEX)
                    .long(ARG_EXCLUDE_FILE_REGEX)
                    .help("exclude files that should not be copied")
                    .takes_value(true)
                    .required(false),
            )
            .arg(
                Arg::with_name(ARG_TOTAL_SIZE_LIMIT)
                    .long(ARG_TOTAL_SIZE_LIMIT)
                    .help("total size of all files in all commits")
                    .takes_value(true)
                    .required(false),
            )
            .arg(
                Arg::with_name(ARG_LFS_THRESHOLD)
                    .long(ARG_LFS_THRESHOLD)
                    .help(
                        "lfs threshold - files with size above that are excluded from file size limit",
                    )
                    .takes_value(true)
                    .required(false),
            )
            .arg(
                Arg::with_name(ARG_OVERWRITE)
                    .long(ARG_OVERWRITE)
                    .help("overwrite a file if it exists in the destination directory")
                    .takes_value(false)
                    .required(false),
            )
        ))
        .subcommand(
            add_common_args(SubCommand::with_name(SUBCOMMAND_REMOVE_EXCESSIVE_FILES)
                .about("remove files from --to directory that are not present in --from directory"))
        )
}

pub fn add_common_args<'a, 'b>(sub_m: App<'a, 'b>) -> App<'a, 'b> {
    sub_m
        .arg(
            Arg::with_name(ARG_FROM_DIR)
                .long(ARG_FROM_DIR)
                .takes_value(true)
                .required(false)
                .help(
                    "name of the directory to copy from. \
                       Error is return if this path doesn't exist or if it's a file",
                ),
        )
        .arg(
            Arg::with_name(ARG_TO_DIR)
                .long(ARG_TO_DIR)
                .takes_value(true)
                .required(false)
                .help(
                    "name of the directory to copy to. \
                       Error is return if this path is a file",
                ),
        )
        .arg(
            Arg::with_name(ARG_FROM_TO_DIRS)
                .long(ARG_FROM_TO_DIRS)
                .multiple(true)
                .takes_value(true)
                .required(false)
                .help("'from_dir=to_dir' directories that needs copying")
                .conflicts_with_all(&[ARG_FROM_DIR, ARG_TO_DIR]),
        )
        .arg(
            Arg::with_name(ARG_COMMIT_MESSAGE)
                .long(ARG_COMMIT_MESSAGE)
                .help("commit message to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(ARG_COMMIT_AUTHOR)
                .long(ARG_COMMIT_AUTHOR)
                .help("commit author to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(ARG_TOTAL_FILE_NUM_LIMIT)
                .long(ARG_TOTAL_FILE_NUM_LIMIT)
                .help("limit the number of files moved in all commits")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_SOURCE_CSID)
                .long(ARG_SOURCE_CSID)
                .takes_value(true)
                .required(true)
                .help("source {hg|bonsai} changeset id or bookmark name"),
        )
        .arg(
            Arg::with_name(ARG_TARGET_CSID)
                .long(ARG_TARGET_CSID)
                .takes_value(true)
                .required(true)
                .help("target {hg|bonsai} changeset id or bookmark name"),
        )
}

/// Get source_cs_id, target_cs_id, from_dir, to_dir, author and commit_msg
/// from subcommand matches
async fn parse_common_args<'a>(
    ctx: &'a CoreContext,
    matches: &'a ArgMatches<'_>,
    source_repo: &'a BlobRepo,
    target_repo: &'a BlobRepo,
) -> Result<
    (
        ChangesetId,
        ChangesetId,
        Vec<(MPath, MPath)>,
        String,
        String,
    ),
    Error,
> {
    let source_cs_id = matches
        .value_of(ARG_SOURCE_CSID)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_SOURCE_CSID))?;

    let target_cs_id = matches
        .value_of(ARG_TARGET_CSID)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_TARGET_CSID))?;

    let (source_cs_id, target_cs_id) = try_join(
        async {
            helpers::csid_resolve(ctx, source_repo, source_cs_id)
                .await
                .context("failed resolving source_cs_id")
        },
        async {
            helpers::csid_resolve(ctx, target_repo, target_cs_id)
                .await
                .context("failed resolving target_cs_id")
        },
    )
    .await?;

    let from_to_dirs = if let Some(from_to_dirs) = matches.value_of(ARG_FROM_TO_DIRS) {
        let mut res = vec![];
        for from_to in from_to_dirs.split(',') {
            let dirs = from_to.split('=').collect::<Vec<_>>();
            if dirs.len() != 2 {
                return Err(anyhow!("invalid format of {}", ARG_FROM_TO_DIRS));
            }
            res.push((MPath::new(dirs[0])?, MPath::new(dirs[1])?));
        }

        res
    } else {
        let from_dir = matches
            .value_of(ARG_FROM_DIR)
            .ok_or_else(|| anyhow!("{} arg is not specified", ARG_FROM_DIR))?;
        let from_dir = MPath::new(from_dir)?;

        let to_dir = matches
            .value_of(ARG_TO_DIR)
            .ok_or_else(|| anyhow!("{} arg is not specified", ARG_TO_DIR))?;
        let to_dir = MPath::new(to_dir)?;

        vec![(from_dir, to_dir)]
    };

    let author = matches
        .value_of(ARG_COMMIT_AUTHOR)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_COMMIT_AUTHOR))?;

    let msg = matches
        .value_of(ARG_COMMIT_MESSAGE)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_COMMIT_MESSAGE))?;

    Ok((
        source_cs_id,
        target_cs_id,
        from_to_dirs,
        author.to_string(),
        msg.to_string(),
    ))
}

pub async fn subcommand_rsync<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let (source_repo, target_repo, _) =
        get_source_target_repos_and_mapping(fb, logger, matches).await?;

    match sub_matches.subcommand() {
        (SUBCOMMAND_COPY, Some(sub_matches)) => {
            let (source_cs_id, target_cs_id, from_to_dirs, author, msg) =
                parse_common_args(&ctx, sub_matches, &source_repo, &target_repo).await?;
            let cs_ids = copy(
                &ctx,
                &source_repo,
                &target_repo,
                source_cs_id,
                target_cs_id,
                from_to_dirs,
                author,
                msg,
                limits_from_matches(sub_matches),
                options_from_matches(sub_matches)?,
                Some(
                    &(|ctx: &CoreContext, path| {
                        warn!(
                            ctx.logger(),
                            "skipping {} because it already exists in the destination, \
                            use --overwrite to override this behaviour",
                            path
                        );
                    }),
                ),
            )
            .await?;

            let result_cs_id = cs_ids
                .last()
                .copied()
                .ok_or_else(|| anyhow!("nothing to move!"))?;

            println!("{}", result_cs_id);
        }
        (SUBCOMMAND_REMOVE_EXCESSIVE_FILES, Some(sub_matches)) => {
            let (source_cs_id, target_cs_id, from_to_dirs, author, msg) =
                parse_common_args(&ctx, sub_matches, &source_repo, &target_repo).await?;

            let maybe_total_file_num_limit: Option<NonZeroU64> =
                args::get_and_parse_opt(sub_matches, ARG_TOTAL_FILE_NUM_LIMIT);

            let result_cs_id = remove_excessive_files(
                &ctx,
                &source_repo,
                &target_repo,
                source_cs_id,
                target_cs_id,
                from_to_dirs,
                author,
                msg,
                maybe_total_file_num_limit,
            )
            .await?;

            println!("{}", result_cs_id);
        }
        _ => return Err(SubcommandError::InvalidArgs),
    }

    Ok(())
}

fn limits_from_matches(sub_m: &ArgMatches<'_>) -> Limits {
    let maybe_total_file_num_limit: Option<NonZeroU64> =
        args::get_and_parse_opt(sub_m, ARG_TOTAL_FILE_NUM_LIMIT);
    let maybe_total_size_limit: Option<NonZeroU64> =
        args::get_and_parse_opt(sub_m, ARG_TOTAL_SIZE_LIMIT);
    let maybe_lfs_threshold: Option<NonZeroU64> = args::get_and_parse_opt(sub_m, ARG_LFS_THRESHOLD);

    Limits {
        total_file_num_limit: maybe_total_file_num_limit,
        total_size_limit: maybe_total_size_limit,
        lfs_threshold: maybe_lfs_threshold,
    }
}

fn options_from_matches(sub_m: &ArgMatches<'_>) -> Result<Options, Error> {
    let maybe_exclude_file_regex = sub_m.value_of(ARG_EXCLUDE_FILE_REGEX);
    let maybe_exclude_file_regex = maybe_exclude_file_regex
        .map(Regex::new)
        .transpose()
        .map_err(Error::from)?;

    let overwrite = sub_m.is_present(ARG_OVERWRITE);

    Ok(Options {
        maybe_exclude_file_regex,
        overwrite,
    })
}
