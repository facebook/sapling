/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{bail, Context, Result};
use bookmarks::{BookmarkName, BookmarkUpdateReason, Bookmarks};
use clap::{Arg, SubCommand};
use cmdlib::args::{self, MononokeClapApp};
use context::CoreContext;
use dbbookmarks::SqlBookmarksBuilder;
use fbinit::FacebookInit;
use mononoke_types::ChangesetId;
use std::str::FromStr;

const CREATE: &str = "create";
const UPDATE: &str = "update";

const BOOKMARK: &str = "bookmark";
const BLOBIMPORT: &str = "blobimport";

const FROM_ID: &str = "from_id";
const TO_ID: &str = "to_id";
const ID: &str = "id";

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    let create = SubCommand::with_name(CREATE).arg(Arg::with_name(ID).required(true));

    let update = SubCommand::with_name(UPDATE)
        .arg(Arg::with_name(FROM_ID).required(true))
        .arg(Arg::with_name(TO_ID).required(true));

    args::MononokeAppBuilder::new("Insert stub log entries - use to test e.g. the admin tool")
        .with_advanced_args_hidden()
        .build()
        .arg(
            Arg::with_name(BOOKMARK)
                .long(BOOKMARK)
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(BLOBIMPORT)
                .long(BLOBIMPORT)
                .required(false)
                .help("Use blobimport reason"),
        )
        .subcommand(create)
        .subcommand(update)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let matches = setup_app().get_matches(fb)?;
    let matches = &matches;

    let fut = async move {
        let config_store = matches.config_store();
        let repo_id = args::get_repo_id(config_store, matches)?;
        let builder = args::open_sql::<SqlBookmarksBuilder>(fb, config_store, matches)?;
        let bookmarks = builder.with_repo_id(repo_id);
        let name = matches
            .value_of(BOOKMARK)
            .context("no bookmark")?
            .to_string();
        let reason = match matches.is_present(BLOBIMPORT) {
            true => BookmarkUpdateReason::Blobimport,
            false => BookmarkUpdateReason::TestMove,
        };

        let bookmark = BookmarkName::new(name)?;

        let mut txn = bookmarks.create_transaction(ctx);

        match matches.subcommand() {
            (CREATE, Some(sub_m)) => {
                txn.create(
                    &bookmark,
                    ChangesetId::from_str(&sub_m.value_of(ID).context("no ID")?.to_string())?,
                    reason,
                    None,
                )?;
            }
            (UPDATE, Some(sub_m)) => {
                txn.update(
                    &bookmark,
                    ChangesetId::from_str(&sub_m.value_of(TO_ID).context("no TO_ID")?.to_string())?,
                    ChangesetId::from_str(
                        &sub_m.value_of(FROM_ID).context("no FROM_ID")?.to_string(),
                    )?,
                    reason,
                    None,
                )?;
            }
            _ => {
                bail!("{}", matches.usage());
            }
        }

        txn.commit().await?;

        Ok(())
    };

    matches.runtime().block_on(fut)?;

    Ok(())
}
