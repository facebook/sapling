/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::ArgMatches;
use fbinit::FacebookInit;
use futures_ext::FutureExt;
use futures_old::prelude::*;
use futures_preview::compat::Future01CompatExt;
use std::str::FromStr;

use cmdlib::args;
use context::CoreContext;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use slog::Logger;

use crate::error::SubcommandError;

pub async fn subcommand_hash_convert<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let source_hash = sub_m.value_of("HASH").unwrap().to_string();
    let source = sub_m.value_of("from").unwrap().to_string();
    let target = sub_m.value_of("to").unwrap();
    // Check that source and target are different types.
    assert_eq!(
        false,
        (source == "hg") ^ (target == "bonsai"),
        "source and target should be different"
    );
    args::init_cachelib(fb, &matches, None);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    args::open_repo(fb, &logger, &matches)
        .and_then(move |repo| {
            if source == "hg" {
                repo.get_bonsai_from_hg(
                    ctx,
                    HgChangesetId::from_str(&source_hash)
                        .expect("source hash is not valid hg changeset id"),
                )
                .and_then(move |maybebonsai| {
                    match maybebonsai {
                        Some(bonsai) => {
                            println!("{}", bonsai);
                        }
                        None => {
                            panic!("no matching mononoke id found");
                        }
                    }
                    Ok(())
                })
                .left_future()
            } else {
                repo.get_hg_from_bonsai_changeset(
                    ctx,
                    ChangesetId::from_str(&source_hash)
                        .expect("source hash is not valid mononoke id"),
                )
                .and_then(move |mercurial| {
                    println!("{}", mercurial);
                    Ok(())
                })
                .right_future()
            }
        })
        .from_err()
        .compat()
        .await
}
