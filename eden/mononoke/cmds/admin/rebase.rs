/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use blobstore::Loadable;
use clap::{App, Arg, ArgMatches, SubCommand};
use fbinit::FacebookInit;
use futures::{compat::Future01CompatExt, future::try_join};

use blobrepo::save_bonsai_changesets;
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::CoreContext;
use mononoke_types::{BonsaiChangesetMut, ChangesetId};
use slog::Logger;

use crate::error::SubcommandError;

pub const ARG_DEST: &str = "dest";
pub const ARG_CSID: &str = "csid";
pub const REBASE: &str = "rebase";
const ARG_I_KNOW: &str = "i-know-what-i-am-doing";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(REBASE)
        .about(
            "produce a bonsai changeset clone with p1 changed to a given value. \
             DOES NOT RUN ANY SAFETY CHECKS, DOES NOT CHECK FOR CONFLICTS!",
        )
        .arg(
            Arg::with_name(ARG_CSID)
                .long(ARG_CSID)
                .takes_value(true)
                .required(true)
                .help("{hg|bonsai} changeset id or bookmark name"),
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
    for maybe_file_change in bcs.file_changes.values_mut() {
        if let Some((_, ref mut cs_id)) = maybe_file_change.as_mut().and_then(|c| c.copy_from_mut())
        {
            *cs_id = new_parent;
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

    args::init_cachelib(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::open_repo(fb, &logger, &matches).await?;

    let cs_id = sub_matches
        .value_of(ARG_CSID)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_CSID))?;

    let dest = sub_matches
        .value_of(ARG_DEST)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_DEST))?;

    let (cs_id, dest) = try_join(
        helpers::csid_resolve(ctx.clone(), repo.clone(), cs_id).compat(),
        helpers::csid_resolve(ctx.clone(), repo.clone(), dest).compat(),
    )
    .await?;

    let bcs = cs_id
        .load(&ctx, repo.blobstore())
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

    println!("{}", rebased_cs_id);
    Ok(())
}
