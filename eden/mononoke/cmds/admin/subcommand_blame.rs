/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::error::SubcommandError;

use anyhow::{format_err, Error};
use blame::{fetch_blame, fetch_file_full_content, BlameRoot};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bounded_traversal::{bounded_traversal_dag, Iter};
use bytes::Bytes;
use clap::{App, Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::CoreContext;
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::{ready, try_join, try_join_all},
    Future, FutureExt, StreamExt, TryFutureExt, TryStreamExt,
};
use manifest::ManifestOps;
use mononoke_types::{
    blame::{Blame, BlameMaybeRejected, BlameRejected},
    BlameId, ChangesetId, FileUnodeId, MPath,
};
use slog::Logger;
use std::collections::HashMap;
use std::fmt::Write;
use unodes::RootUnodeManifestId;

pub const BLAME: &str = "blame";
const COMMAND_DERIVE: &str = "derive";
const COMMAND_COMPUTE: &str = "compute";
const COMMAND_DIFF: &str = "diff";
const COMMAND_FIND_REJECTED: &str = "find-rejected";

const ARG_CSID: &str = "csid";
const ARG_PATH: &str = "path";
const ARG_PRINT_ERRORS: &str = "print-errors";
const ARG_LINE: &str = "line";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    let csid_arg = Arg::with_name(ARG_CSID)
        .help("{hg|bonsai} changeset id or bookmark name")
        .index(1)
        .required(true);
    let path_arg = Arg::with_name(ARG_PATH)
        .help("path")
        .index(2)
        .required(true);
    let line_number_arg = Arg::with_name(ARG_LINE)
        .help("show line number at the first appearance")
        .short("l")
        .long("line-number")
        .takes_value(false)
        .required(false);

    SubCommand::with_name(BLAME)
        .about("fetch/derive blame for specified changeset and path")
        .subcommand(
            SubCommand::with_name(COMMAND_DERIVE)
                .arg(line_number_arg.clone())
                .arg(csid_arg.clone())
                .arg(path_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_DIFF)
                .arg(csid_arg.clone())
                .arg(path_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_COMPUTE)
                .arg(line_number_arg.clone())
                .arg(csid_arg.clone())
                .arg(path_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_FIND_REJECTED)
                .arg(csid_arg.clone())
                .arg(
                    Arg::with_name(ARG_PRINT_ERRORS)
                        .help("print why the file is rejected")
                        .long(ARG_PRINT_ERRORS)
                        .takes_value(false)
                        .required(false),
                ),
        )
}

pub async fn subcommand_blame<'a>(
    fb: FacebookInit,
    logger: Logger,
    toplevel_matches: &'a MononokeMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    args::init_cachelib(fb, toplevel_matches);

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    match sub_matches.subcommand() {
        (COMMAND_DERIVE, Some(matches)) => {
            let repo = args::open_repo(fb, &logger, toplevel_matches).await?;
            let line_number = matches.is_present(ARG_LINE);
            with_changeset_and_path(ctx, repo, matches, move |ctx, repo, csid, path| {
                subcommand_show_blame(ctx, repo, csid, path, line_number)
            })
            .await
        }
        (COMMAND_DIFF, Some(matches)) => {
            let repo = args::open_repo(fb, &logger, toplevel_matches).await?;
            with_changeset_and_path(ctx, repo, matches, subcommand_show_diffs).await
        }
        (COMMAND_COMPUTE, Some(matches)) => {
            let repo = args::open_repo(fb, &logger, toplevel_matches).await?;
            let line_number = matches.is_present(ARG_LINE);
            with_changeset_and_path(ctx, repo, matches, move |ctx, repo, csid, path| {
                subcommand_compute_blame(ctx, repo, csid, path, line_number)
            })
            .await
        }
        (COMMAND_FIND_REJECTED, Some(matches)) => {
            let print_errors = matches.is_present(ARG_PRINT_ERRORS);
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let repo = args::open_repo(fb, &logger, toplevel_matches).await?;
            let cs_id = helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                .compat()
                .await?;

            let derived_unode = RootUnodeManifestId::derive(&ctx, &repo, cs_id)
                .map_err(Error::from)
                .await?;

            let mut paths = derived_unode
                .manifest_unode_id()
                .list_leaf_entries(ctx.clone(), repo.get_blobstore())
                .map_ok(|(path, file_unode_id)| {
                    let id = BlameId::from(file_unode_id);
                    cloned!(ctx, repo);
                    async move { id.load(&ctx, repo.blobstore()).await }
                        .map_ok(move |blame_maybe_rejected| (path, blame_maybe_rejected))
                        .map_err(Error::from)
                })
                .try_buffer_unordered(100)
                .try_filter_map(|(path, blame_maybe_rejected)| async move {
                    match blame_maybe_rejected {
                        BlameMaybeRejected::Rejected(rejected) => Ok(Some((path, rejected))),
                        BlameMaybeRejected::Blame(_) => Ok(None),
                    }
                })
                .boxed();

            while let Some((p, rejected)) = paths.try_next().await? {
                if print_errors {
                    println!("{} {}", p, rejected);
                } else {
                    println!("{}", p);
                }
            }
            return Ok(());
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn with_changeset_and_path<F, FOut>(
    ctx: CoreContext,
    repo: BlobRepo,
    matches: &ArgMatches<'_>,
    fun: F,
) -> Result<(), SubcommandError>
where
    F: FnOnce(CoreContext, BlobRepo, ChangesetId, MPath) -> FOut + Send + 'static,
    FOut: Future<Output = Result<(), Error>> + Send + 'static,
{
    let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
    let path = MPath::new(matches.value_of(ARG_PATH).unwrap())?;
    let csid = helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
        .compat()
        .await?;
    fun(ctx, repo, csid, path).await?;
    Ok(())
}

async fn subcommand_show_blame(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
    line_number: bool,
) -> Result<(), Error> {
    let (content, blame) = fetch_blame(&ctx, &repo, csid, path).await?;
    let annotate = blame_hg_annotate(ctx, repo, content, blame, line_number).await?;
    println!("{}", annotate);
    Ok(())
}

async fn find_leaf(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> Result<FileUnodeId, Error> {
    let mf_root = RootUnodeManifestId::derive(&ctx, &repo, csid).await?;
    let entry_opt = mf_root
        .manifest_unode_id()
        .clone()
        .find_entry(ctx, repo.get_blobstore(), Some(path.clone()))
        .await?;
    let entry = entry_opt.ok_or_else(|| format_err!("No such path: {}", path))?;
    match entry.into_leaf() {
        None => Err(format_err!(
            "Blame is not available for directories: {}",
            path
        )),
        Some(file_unode_id) => Ok(file_unode_id),
    }
}

async fn subcommand_show_diffs(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> Result<(), Error> {
    let file_unode_id = find_leaf(ctx.clone(), repo.clone(), csid, path).await?;
    let file_unode = file_unode_id.load(&ctx, repo.blobstore()).await?;
    let diffs = file_unode
        .parents()
        .iter()
        .map(|parent| diff(ctx.clone(), repo.clone(), file_unode_id, *parent));
    let diffs = try_join_all(diffs).await?;
    for diff in diffs {
        println!("{}", diff);
    }
    Ok(())
}

async fn diff(
    ctx: CoreContext,
    repo: BlobRepo,
    new: FileUnodeId,
    old: FileUnodeId,
) -> Result<String, Error> {
    let options = BlameRoot::default_mapping(&ctx, &repo)?.options();
    let f1 = fetch_file_full_content(&ctx, &repo, new, options)
        .and_then(|result| ready(result.map_err(Error::from)));
    let f2 = fetch_file_full_content(&ctx, &repo, old, options)
        .and_then(|result| ready(result.map_err(Error::from)));
    let (new, old) = try_join(f1, f2).await?;
    let new = xdiff::DiffFile {
        path: "new",
        contents: xdiff::FileContent::Inline(new),
        file_type: xdiff::FileType::Regular,
    };
    let old = xdiff::DiffFile {
        path: "old",
        contents: xdiff::FileContent::Inline(old),
        file_type: xdiff::FileType::Regular,
    };
    let diff = xdiff::diff_unified(
        Some(old),
        Some(new),
        xdiff::DiffOpts {
            context: 3,
            copy_info: xdiff::CopyInfo::None,
        },
    );
    Ok(String::from_utf8_lossy(&diff).into_owned())
}

/// Recalculate balme by going through whole history of a file
async fn subcommand_compute_blame(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
    line_number: bool,
) -> Result<(), Error> {
    let blobstore = repo.get_blobstore().boxed();
    let blame_mapping = BlameRoot::default_mapping(&ctx, &repo)?;
    let file_unode_id = find_leaf(ctx.clone(), repo.clone(), csid, path.clone()).await?;
    let blame_options = blame_mapping.options();
    let (content, blame) = bounded_traversal_dag(
        256,
        (file_unode_id, path),
        {
            // unfold operator traverses all parents of a given unode, accounting for
            // renames and treating them as another parent.
            //
            |(file_unode_id, path)| {
                cloned!(ctx, repo, blobstore);
                async move {
                    let file_unode = file_unode_id.load(&ctx, &blobstore).await?;
                    let csid = *file_unode.linknode();
                    let bonsai = csid.load(&ctx, &blobstore).await?;
                    let copy_from = bonsai
                        .file_changes_map()
                        .get(&path)
                        .and_then(|file_change| Some(file_change.as_ref()?.copy_from().clone()?));
                    let copy_parent: Option<(FileUnodeId, MPath)> = match copy_from {
                        None => None,
                        Some((r_path, r_csid)) => {
                            let r_unode_id =
                                find_leaf(ctx.clone(), repo, *r_csid, r_path.clone()).await?;
                            Some((r_unode_id, r_path.clone()))
                        }
                    };
                    let parents: Vec<_> = file_unode
                        .parents()
                        .iter()
                        .map(|unode_id| (*unode_id, path.clone()))
                        .chain(copy_parent)
                        .collect();
                    Ok(((csid, path, file_unode_id), parents))
                }
                .boxed()
            }
        },
        {
            |(csid, path, file_unode_id), parents: Iter<Result<(Bytes, Blame), BlameRejected>>| {
                cloned!(ctx, repo);
                async move {
                    let content =
                        fetch_file_full_content(&ctx, &repo, file_unode_id, blame_options).await?;
                    match content {
                        Err(rejected) => Ok(Err(rejected)),
                        Ok(content) => {
                            let parents = parents
                                .into_iter()
                                .filter_map(|result| result.ok())
                                .collect();
                            Blame::from_parents(csid, content.clone(), path.clone(), parents)
                                .map(move |blame| Ok((content, blame)))
                        }
                    }
                }
                .boxed()
            }
        },
    )
    .await?
    .ok_or_else(|| Error::msg("cycle found"))??;
    let annotate = blame_hg_annotate(ctx, repo, content, blame, line_number).await?;
    println!("{}", annotate);
    Ok(())
}

/// Format blame the same way `hg blame` does
async fn blame_hg_annotate<C: AsRef<[u8]> + 'static + Send>(
    ctx: CoreContext,
    repo: BlobRepo,
    content: C,
    blame: Blame,
    show_line_number: bool,
) -> Result<String, Error> {
    if content.as_ref().is_empty() {
        return Ok(String::new());
    }

    let csids: Vec<_> = blame.ranges().iter().map(|range| range.csid).collect();
    let mapping = repo.get_hg_bonsai_mapping(ctx, csids).await?;
    let mapping: HashMap<_, _> = mapping.into_iter().map(|(k, v)| (v, k)).collect();

    let content = String::from_utf8_lossy(content.as_ref());
    let mut result = String::new();
    for (line, (csid, _path, line_number)) in content.lines().zip(blame.lines()) {
        let hg_csid = mapping
            .get(&csid)
            .ok_or_else(|| format_err!("unresolved bonsai csid: {}", csid))?;
        result.push_str(&hg_csid.to_string()[..12]);
        result.push(':');
        if show_line_number {
            write!(&mut result, "{:>4}:", line_number + 1)?;
        }
        result.push(' ');
        result.push_str(line);
        result.push('\n');
    }

    Ok(result)
}
