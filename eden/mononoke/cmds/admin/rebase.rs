/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobstore::Loadable;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use fbinit::FacebookInit;
use futures::compat::Stream01CompatExt;
use futures::future::try_join;
use futures::future::try_join3;
use futures::TryStreamExt;

use blobrepo::save_bonsai_changesets;
use blobrepo::BlobRepo;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use slog::Logger;

use crate::error::SubcommandError;

pub const ARG_DEST: &str = "dest";
pub const ARG_CSID: &str = "csid";
pub const ARG_REBASE_ANCESTOR: &str = "rebase-ancestor";
pub const ARG_REBASE_DESCENDANT: &str = "rebase-descendant";
pub const REBASE: &str = "rebase";
const ARG_I_KNOW: &str = "i-know-what-i-am-doing";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(REBASE)
        .about(
            "rebases a single commit or a stack onto a given destination. \
             DOES NOT RUN ANY SAFETY CHECKS, DOES NOT CHECK FOR CONFLICTS!",
        )
        .arg(
            Arg::with_name(ARG_CSID)
                .long(ARG_CSID)
                .takes_value(true)
                .required(false)
                .conflicts_with_all(&[ARG_REBASE_ANCESTOR, ARG_REBASE_DESCENDANT])
                .help("{hg|bonsai} changeset id or bookmark name to rebase"),
        )
        .arg(
            Arg::with_name(ARG_REBASE_ANCESTOR)
                .long(ARG_REBASE_ANCESTOR)
                .takes_value(true)
                .required(false)
                .conflicts_with(ARG_CSID)
                .help("ancestor of the stack of commits to rebase"),
        )
        .arg(
            Arg::with_name(ARG_REBASE_DESCENDANT)
                .long(ARG_REBASE_DESCENDANT)
                .takes_value(true)
                .required(false)
                .conflicts_with(ARG_CSID)
                .help("descendant of the stack of commits to rebase"),
        )
        .arg(
            Arg::with_name(ARG_DEST)
                .long(ARG_DEST)
                .takes_value(true)
                .required(true)
                .help("desired value of the p1"),
        )
        .arg(
            Arg::with_name(ARG_I_KNOW)
                .long(ARG_I_KNOW)
                .takes_value(false)
                .help("Acknowledges that you understnad that this is an unsafe command"),
        )
}

fn copyfrom_fixup(bcs: &mut BonsaiChangesetMut, new_parent: ChangesetId) {
    for file_change in bcs.file_changes.values_mut() {
        match file_change {
            FileChange::Change(fc) => {
                if let Some((_, ref mut csid)) = fc.copy_from_mut() {
                    *csid = new_parent;
                }
            }
            FileChange::Deletion
            | FileChange::UntrackedDeletion
            | FileChange::UntrackedChange(_) => {}
        }
    }
}

pub async fn subcommand_rebase<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    if !sub_matches.is_present(ARG_I_KNOW) {
        return Err(anyhow!("{} is required", ARG_I_KNOW).into());
    }

    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo: BlobRepo = args::open_repo(fb, &logger, matches).await?;

    let dest = sub_matches
        .value_of(ARG_DEST)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_DEST))?;

    if let Some(cs_id) = sub_matches.value_of(ARG_CSID) {
        let (cs_id, dest) = try_join(
            helpers::csid_resolve(&ctx, &repo, cs_id),
            helpers::csid_resolve(&ctx, &repo, dest),
        )
        .await?;

        let rebased_cs_id = rebase_single_changeset(&ctx, &repo, cs_id, dest).await?;

        println!("{}", rebased_cs_id);
        return Ok(());
    }

    let ancestor = sub_matches
        .value_of(ARG_REBASE_ANCESTOR)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_REBASE_ANCESTOR))?;
    let descendant = sub_matches
        .value_of(ARG_REBASE_DESCENDANT)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_REBASE_DESCENDANT))?;

    let (ancestor, descendant, dest) = try_join3(
        helpers::csid_resolve(&ctx, &repo, ancestor),
        helpers::csid_resolve(&ctx, &repo, descendant),
        helpers::csid_resolve(&ctx, &repo, dest),
    )
    .await?;

    let ctx = &ctx;
    let cs_fetcher = &repo.get_changeset_fetcher();
    let csids = revset::RangeNodeStream::new(ctx.clone(), cs_fetcher.clone(), ancestor, descendant)
        .compat()
        .map_ok(|csid| async move {
            let parents = cs_fetcher.get_parents(ctx.clone(), csid).await?;
            if parents.len() > 1 {
                return Err(anyhow!("rebasing stack with merges is not supported"));
            }
            Ok(csid)
        })
        .try_buffered(100)
        .try_collect::<Vec<_>>()
        .await?;

    // Reverse since we want to iterate from ancestors to descendants
    let iter = csids.into_iter().rev();
    let mut dest = dest;
    for csid in iter {
        let rebased_cs_id = rebase_single_changeset(ctx, &repo, csid, dest).await?;
        println!("{}", rebased_cs_id);
        dest = rebased_cs_id;
    }

    Ok(())
}

async fn rebase_single_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    dest: ChangesetId,
) -> Result<ChangesetId, Error> {
    let bcs = cs_id
        .load(ctx, repo.blobstore())
        .await
        .map_err(Error::from)?;
    let mut rebased = bcs.into_mut();

    if rebased.parents.is_empty() {
        rebased.parents.push(dest);
    } else {
        rebased.parents[0] = dest;
    }

    copyfrom_fixup(&mut rebased, dest);

    let rebased = rebased.freeze()?;
    let rebased_cs_id = rebased.get_changeset_id();
    save_bonsai_changesets(vec![rebased], ctx.clone(), repo).await?;
    Ok(rebased_cs_id)
}
