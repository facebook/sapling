/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cmdlib::args;

use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use cloned::cloned;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use fbinit::FacebookInit;
use filenodes::FilenodeInfo;
use futures::future::try_join_all;
use futures::TryStreamExt;
use futures_stats::TimedFutureExt;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgFileEnvelope;
use mercurial_types::HgFileNodeId;
use mercurial_types::MPath;
use mononoke_types::RepoPath;
use slog::debug;
use slog::info;
use slog::Logger;

use crate::common::get_file_nodes;
use crate::error::SubcommandError;

pub const FILENODES: &str = "filenodes";
const COMMAND_ID: &str = "by-id";
const COMMAND_REVISION: &str = "by-revision";
const COMMAND_VALIDATE: &str = "validate";
const COMMAND_ALL_FILENODES: &str = "all-filenodes";

const ARG_ENVELOPE: &str = "envelope";
const ARG_IS_TREE: &str = "is-tree";

const ARG_REVISION: &str = "changeset-id";
const ARG_PATHS: &str = "paths";

const ARG_ID: &str = "id";
const ARG_PATH: &str = "path";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(FILENODES)
        .about("fetches hg filenodes information for a commit and one or more paths")
        .arg(
            Arg::with_name(ARG_ENVELOPE)
                .long(ARG_ENVELOPE)
                .required(false)
                .takes_value(false)
                .help("whether to show the envelope as well"),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_REVISION)
                .arg(
                    Arg::with_name(ARG_REVISION)
                        .required(true)
                        .takes_value(true)
                        .help("hg/bonsai changeset id or bookmark to lookup filenodes for"),
                )
                .arg(
                    Arg::with_name(ARG_PATHS)
                        .required(true)
                        .multiple(true)
                        .takes_value(true)
                        .help("a list of file paths to lookup filenodes for"),
                ),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_ID)
                .arg(
                    Arg::with_name(ARG_PATH)
                        .required(true)
                        .takes_value(true)
                        .help("path to lookup filenode for (use trailing / for directories)"),
                )
                .arg(
                    Arg::with_name(ARG_ID)
                        .required(true)
                        .takes_value(true)
                        .help("filenode ID"),
                ),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_VALIDATE)
                .about("for a public changeset validates that all files and trees exist")
                .arg(
                    Arg::with_name(ARG_REVISION)
                        .required(true)
                        .takes_value(true)
                        .help("hg/bonsai changeset id or bookmark"),
                ),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_ALL_FILENODES)
                .about("print all filenodes for a path")
                .arg(
                    Arg::with_name(ARG_PATH)
                        .long(ARG_PATH)
                        .required(true)
                        .takes_value(true)
                        .help("path of the filenode"),
                )
                .arg(
                    Arg::with_name(ARG_IS_TREE)
                        .long(ARG_IS_TREE)
                        .required(false)
                        .takes_value(false)
                        .help("whether it's a tree"),
                ),
        )
}

fn extract_path(path: &str) -> Result<MPath, Error> {
    MPath::new(path).map_err(|err| format_err!("Could not parse path {}: {:?}", path, err))
}

fn log_filenode(
    logger: &Logger,
    path: &RepoPath,
    filenode: &FilenodeInfo,
    envelope: Option<&HgFileEnvelope>,
) -> Option<()> {
    let FilenodeInfo {
        filenode,
        p1,
        p2,
        copyfrom,
        linknode,
    } = filenode;

    info!(logger, "Filenode: {:?}", filenode);
    info!(logger, "-- path: {:?}", path);
    info!(logger, "-- p1: {:?}", p1);
    info!(logger, "-- p2: {:?}", p2);
    info!(logger, "-- copyfrom: {:?}", copyfrom);
    info!(logger, "-- linknode: {:?}", linknode);
    info!(logger, "-- content id: {:?}", envelope?.content_id());
    info!(logger, "-- content size: {:?}", envelope?.content_size());
    info!(logger, "-- metadata: {:?}", envelope?.metadata());
    Some(())
}

async fn handle_filenodes_at_revision(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    revision: &str,
    paths: Vec<MPath>,
    log_envelope: bool,
) -> Result<(), Error> {
    let cs_id = helpers::csid_resolve(&ctx, &blobrepo, revision.to_string()).await?;
    let cs_id = blobrepo.derive_hg_changeset(&ctx, cs_id).await?;
    let filenode_ids = get_file_nodes(
        ctx.clone(),
        ctx.logger().clone(),
        &blobrepo,
        cs_id,
        paths.clone(),
    )
    .await?;
    let logger = ctx.logger().clone();
    let filenodes_futs =
        paths
            .into_iter()
            .zip(filenode_ids.into_iter())
            .map(move |(path, filenode_id)| {
                cloned!(ctx, blobrepo);
                async move {
                    let path = RepoPath::FilePath(path);

                    let filenode_fut = async {
                        blobrepo
                            .get_filenode(ctx.clone(), &path, filenode_id)
                            .await?
                            .do_not_handle_disabled_filenodes()
                    };
                    let envelope_fut = async {
                        if log_envelope {
                            Ok(Some(filenode_id.load(&ctx, blobrepo.blobstore()).await?))
                        } else {
                            Ok(None)
                        }
                    };
                    let (filenode, envelope) = futures::try_join!(filenode_fut, envelope_fut)?;

                    Ok::<_, Error>((path, filenode, envelope))
                }
            });
    let filenodes = try_join_all(filenodes_futs).await?;
    for (path, filenode, envelope) in filenodes {
        log_filenode(&logger, &path, &filenode, envelope.as_ref());
    }
    Ok(())
}

pub async fn subcommand_filenodes<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::open_repo(fb, ctx.logger(), matches).await?;
    let log_envelope = sub_m.is_present(ARG_ENVELOPE);

    match sub_m.subcommand() {
        (COMMAND_REVISION, Some(matches)) => {
            let rev = matches.value_of(ARG_REVISION).unwrap().to_string();

            let paths: Result<Vec<_>, Error> = matches
                .values_of(ARG_PATHS)
                .unwrap()
                .map(extract_path)
                .collect();
            let paths = paths?;

            handle_filenodes_at_revision(ctx, repo, &rev, paths, log_envelope)
                .await
                .map_err(SubcommandError::Error)
        }
        (COMMAND_ID, Some(matches)) => {
            let path = matches.value_of(ARG_PATH).unwrap();
            let path = match path.chars().last() {
                Some('/') => extract_path(path).map(RepoPath::DirectoryPath),
                Some(_) => extract_path(path).map(RepoPath::FilePath),
                None => Ok(RepoPath::RootPath),
            }?;

            let id = matches.value_of(ARG_ID).unwrap().parse::<HgFileNodeId>()?;

            let filenode = repo
                .get_filenode(ctx.clone(), &path, id)
                .await?
                .do_not_handle_disabled_filenodes()?;
            let envelope = if log_envelope {
                let res = filenode
                    .filenode
                    .load(&ctx, repo.blobstore())
                    .await
                    .map_err(Error::from)?;
                Some(res)
            } else {
                None
            };
            log_filenode(ctx.logger(), &path, &filenode, envelope.as_ref());
            Ok(())
        }
        (COMMAND_VALIDATE, Some(matches)) => {
            let rev = matches.value_of(ARG_REVISION).unwrap().to_string();
            let ctx = CoreContext::new_with_logger(fb, logger.clone());

            let mf_id = helpers::get_root_manifest_id(&ctx, repo.clone(), rev).await?;
            mf_id
                .list_all_entries(ctx.clone(), repo.get_blobstore())
                .map_ok(move |(path, entry)| {
                    cloned!(ctx, repo);
                    async move {
                        let (repo_path, filenode_id) = match entry {
                            Entry::Leaf((_, filenode_id)) => (
                                RepoPath::FilePath(path.expect("unexpected empty file path")),
                                filenode_id,
                            ),
                            Entry::Tree(mf_id) => {
                                let filenode_id = HgFileNodeId::new(mf_id.into_nodehash());
                                match path {
                                    Some(path) => (RepoPath::DirectoryPath(path), filenode_id),
                                    None => (RepoPath::RootPath, filenode_id),
                                }
                            }
                        };
                        let maybe_filenode = repo
                            .get_filenode_opt(ctx, &repo_path, filenode_id)
                            .await?
                            .do_not_handle_disabled_filenodes()?;
                        if maybe_filenode.is_some() {
                            Ok(())
                        } else {
                            Err(format_err!("not found filenode for {}", repo_path))
                        }
                    }
                })
                .try_buffer_unordered(100)
                .try_for_each(|_| async { Ok(()) })
                .await?;
            Ok(())
        }
        (COMMAND_ALL_FILENODES, Some(matches)) => {
            let maybe_mpath = MPath::new_opt(matches.value_of(ARG_PATH).unwrap())?;
            let is_tree = matches.is_present(ARG_IS_TREE);
            let path = match (maybe_mpath, is_tree) {
                (Some(path), true) => RepoPath::DirectoryPath(path),
                (Some(path), false) => RepoPath::FilePath(path),
                (None, true) => RepoPath::RootPath,
                (None, false) => {
                    return Err(
                        anyhow!("you need to provide a non-empty path or pass --is-tree").into(),
                    );
                }
            };

            let (stats, res) = repo
                .filenodes()
                .get_all_filenodes_maybe_stale(&ctx, &path, None)
                .timed()
                .await;

            debug!(ctx.logger(), "took {:?}", stats.completion_time);
            let maybe_filenodes = res?.do_not_handle_disabled_filenodes()?;
            let filenodes = maybe_filenodes
                .ok_or_else(|| anyhow!("unexpected failure: history is too long?"))?;
            for filenode in filenodes {
                log_filenode(ctx.logger(), &path, &filenode, None);
            }
            Ok(())
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}
