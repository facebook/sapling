/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::error::SubcommandError;

use anyhow::format_err;
use anyhow::Error;
use blame::fetch_blame_compat;
use blame::fetch_content_for_blame;
use blame::CompatBlame;
use blame::FetchOutcome;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bounded_traversal::bounded_traversal_dag;
use bounded_traversal::Iter;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cloned::cloned;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::Future;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mononoke_types::blame::Blame;
use mononoke_types::blame::BlameMaybeRejected;
use mononoke_types::blame::BlameRejected;
use mononoke_types::blame_v2::BlameParent;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::BlameId;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::FileUnodeId;
use mononoke_types::MPath;
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
const ARG_BLAME_V2: &str = "blame-v2";

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
    let blame_v2_arg = Arg::with_name(ARG_BLAME_V2)
        .help("use blame-v2")
        .long("blame-v2")
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
                .arg(path_arg.clone())
                .arg(blame_v2_arg.clone()),
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
            let blame_v2 = matches.is_present(ARG_BLAME_V2);
            with_changeset_and_path(ctx, repo, matches, move |ctx, repo, csid, path| {
                subcommand_compute_blame(ctx, repo, csid, path, line_number, blame_v2)
            })
            .await
        }
        (COMMAND_FIND_REJECTED, Some(matches)) => {
            let print_errors = matches.is_present(ARG_PRINT_ERRORS);
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let repo: BlobRepo = args::open_repo(fb, &logger, toplevel_matches).await?;
            let cs_id = helpers::csid_resolve(&ctx, repo.clone(), hash_or_bookmark).await?;

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
            Ok(())
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
    let csid = helpers::csid_resolve(&ctx, &repo, hash_or_bookmark).await?;
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
    let (blame, unode_id) = fetch_blame_compat(&ctx, &repo, csid, path.clone()).await?;
    let content = fetch_content_for_blame(&ctx, &repo, unode_id)
        .await?
        .into_bytes()?;
    let annotate = blame_hg_annotate(ctx, repo, content, blame, line_number).await?;
    println!("{}", annotate);
    Ok(())
}

/// Finds a leaf that should exist.  Returns an error if the path is not
/// a file in this changeset.
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

/// Attempts to find a leaf, but returns `None` if the path is not a file.
async fn try_find_leaf(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> Result<Option<FileUnodeId>, Error> {
    let mf_root = RootUnodeManifestId::derive(&ctx, &repo, csid).await?;
    let entry_opt = mf_root
        .manifest_unode_id()
        .clone()
        .find_entry(ctx, repo.get_blobstore(), Some(path.clone()))
        .await?;
    Ok(entry_opt.and_then(|entry| entry.into_leaf()))
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
    let f1 = fetch_content_for_blame(&ctx, &repo, new);
    let f2 = fetch_content_for_blame(&ctx, &repo, old);
    let (new, old) = try_join(f1, f2).await?;
    let new = xdiff::DiffFile {
        path: "new",
        contents: xdiff::FileContent::Inline(new.into_bytes()?),
        file_type: xdiff::FileType::Regular,
    };
    let old = xdiff::DiffFile {
        path: "old",
        contents: xdiff::FileContent::Inline(old.into_bytes()?),
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

#[derive(Clone, Debug)]
enum EitherBlame {
    V1(Blame),
    V2(BlameV2),
}

impl EitherBlame {
    fn compat(self) -> CompatBlame {
        match self {
            EitherBlame::V1(blame) => CompatBlame::V1(BlameMaybeRejected::Blame(blame)),
            EitherBlame::V2(blame) => CompatBlame::V2(blame),
        }
    }
}

/// Recalculate blame by going through whole history of a file
async fn subcommand_compute_blame(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
    line_number: bool,
    blame_v2: bool,
) -> Result<(), Error> {
    let blobstore = repo.get_blobstore().boxed();
    let file_unode_id = find_leaf(ctx.clone(), repo.clone(), csid, path.clone()).await?;
    let (_, _, content, blame) = bounded_traversal_dag(
        256,
        (None, path.clone(), file_unode_id),
        {
            // unfold operator traverses all parents of a given unode, accounting for
            // renames and treating them as another parent.
            //
            |(parent_index, path, file_unode_id)| {
                cloned!(ctx, repo, blobstore);
                async move {
                    let file_unode = file_unode_id.load(&ctx, &blobstore).await?;
                    let csid = *file_unode.linknode();
                    let bonsai = csid.load(&ctx, &blobstore).await?;
                    let mut parents = Vec::new();
                    if bonsai.parents().count() == 1 {
                        // The bonsai changeset only has a single parent, so
                        // we can assume that is where the file came from.
                        for parent_unode_id in file_unode.parents().iter() {
                            parents.push((Some(0), path.clone(), *parent_unode_id));
                        }
                    } else {
                        // We must work out which is the first changeset parent that the
                        // parent unodes came from.
                        let mut parent_indexes = HashMap::new();
                        for (parent_index, parent_csid) in bonsai.parents().enumerate() {
                            if let Some(parent_file_unode_id) =
                                try_find_leaf(ctx.clone(), repo.clone(), parent_csid, path.clone())
                                    .await?
                            {
                                parent_indexes.insert(parent_file_unode_id, parent_index);
                            }
                        }
                        for parent_unode_id in file_unode.parents().iter() {
                            parents.push((
                                parent_indexes.get(parent_unode_id).copied(),
                                path.clone(),
                                *parent_unode_id,
                            ));
                        }
                    }
                    let copy_from = bonsai
                        .file_changes_map()
                        .get(&path)
                        .and_then(|file_change| match file_change {
                            FileChange::Change(tc) => Some(tc.copy_from().clone()?),
                            FileChange::Deletion
                            | FileChange::UntrackedDeletion
                            | FileChange::UntrackedChange(_) => None,
                        });
                    if let Some((r_path, r_csid)) = copy_from {
                        let r_parent_index = bonsai
                            .parents()
                            .position(|csid| csid == *r_csid)
                            .ok_or_else(|| {
                                format_err!(
                                    "commit {} path {} has copy-from with invalid parent {}",
                                    csid,
                                    path,
                                    r_csid,
                                )
                            })?;
                        let r_unode_id =
                            find_leaf(ctx.clone(), repo, *r_csid, r_path.clone()).await?;
                        parents.push((Some(r_parent_index), r_path.clone(), r_unode_id))
                    };
                    Ok::<_, Error>(((csid, parent_index, path, file_unode_id), parents))
                }
                .boxed()
            }
        },
        {
            |(csid, parent_index, path, file_unode_id),
             parents: Iter<
                Result<(Option<usize>, MPath, bytes::Bytes, EitherBlame), BlameRejected>,
            >| {
                cloned!(ctx, repo);
                async move {
                    match fetch_content_for_blame(&ctx, &repo, file_unode_id).await? {
                        FetchOutcome::Rejected(rejected) => Ok(Err(rejected)),
                        FetchOutcome::Fetched(content) => if blame_v2 {
                            let parents = parents
                                .into_iter()
                                .filter_map(|parent| match parent {
                                    Ok((
                                        Some(parent_index),
                                        parent_path,
                                        content,
                                        EitherBlame::V2(blame),
                                    )) => Some(BlameParent::new(
                                        parent_index,
                                        parent_path,
                                        content,
                                        blame,
                                    )),
                                    _ => None,
                                })
                                .collect();
                            Ok(EitherBlame::V2(BlameV2::new(
                                csid,
                                path.clone(),
                                content.clone(),
                                parents,
                            )?))
                        } else {
                            let parents = parents
                                .into_iter()
                                .filter_map(|parent| match parent {
                                    Ok((_, _, content, EitherBlame::V1(blame))) => {
                                        Some((content, blame))
                                    }
                                    _ => None,
                                })
                                .collect();
                            Ok(EitherBlame::V1(Blame::from_parents(
                                csid,
                                content.clone(),
                                path.clone(),
                                parents,
                            )?))
                        }
                        .map(move |blame| Ok((parent_index, path, content, blame))),
                    }
                }
                .boxed()
            }
        },
    )
    .await?
    .ok_or_else(|| Error::msg("cycle found"))??;
    let annotate = blame_hg_annotate(ctx, repo, content, blame.compat(), line_number).await?;
    println!("{}", annotate);
    Ok(())
}

/// Format blame the same way `hg blame` does
async fn blame_hg_annotate<C: AsRef<[u8]> + 'static + Send>(
    ctx: CoreContext,
    repo: BlobRepo,
    content: C,
    blame: CompatBlame,
    show_line_number: bool,
) -> Result<String, Error> {
    if content.as_ref().is_empty() {
        return Ok(String::new());
    }
    let content = String::from_utf8_lossy(content.as_ref());
    let mut result = String::new();
    let csids: Vec<_> = blame
        .changeset_ids()?
        .into_iter()
        .map(|(csid, _)| csid)
        .collect();
    let mapping = repo.get_hg_bonsai_mapping(ctx, csids).await?;
    let mapping: HashMap<_, _> = mapping.into_iter().map(|(k, v)| (v, k)).collect();

    for (line, blame_line) in content.lines().zip(blame.lines()?) {
        let hg_csid = mapping
            .get(&blame_line.changeset_id)
            .ok_or_else(|| format_err!("unresolved bonsai csid: {}", blame_line.changeset_id))?;
        if let Some(changeset_index) = blame_line.changeset_index {
            write!(result, "{:>5} ", format!("#{}", changeset_index + 1))?;
        }
        result.push_str(&hg_csid.to_string()[..12]);
        result.push(':');
        if show_line_number {
            write!(&mut result, "{:>4}:", blame_line.origin_offset + 1)?;
        }
        result.push(' ');
        result.push_str(line);
        result.push('\n');
    }

    Ok(result)
}
