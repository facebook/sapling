/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::{App, ArgMatches, SubCommand};
use cmdlib::args;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures_old::prelude::*;
use mononoke_types::{BonsaiChangeset, ChangesetId, DateTime, FileChange};
use serde_derive::Serialize;
use slog::Logger;
use std::collections::BTreeMap;

use crate::common::{fetch_bonsai_changeset, print_bonsai_changeset};
use crate::error::SubcommandError;

pub const BONSAI_FETCH: &str = "bonsai-fetch";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(BONSAI_FETCH)
        .about("fetches content of the file or manifest from blobrepo")
        .args_from_usage(
            r#"<CHANGESET_ID>    'hg/bonsai id or bookmark to fetch file from'
                          --json            'if provided json will be returned'"#,
        )
}

pub async fn subcommand_bonsai_fetch<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let rev = sub_m.value_of("CHANGESET_ID").unwrap().to_string();

    args::init_cachelib(fb, &matches, None);

    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let json_flag = sub_m.is_present("json");

    args::open_repo(fb, &logger, &matches)
        .and_then(move |blobrepo| fetch_bonsai_changeset(ctx, &rev, &blobrepo))
        .map(move |bcs| {
            if json_flag {
                match serde_json::to_string(&SerializableBonsaiChangeset::from(bcs)) {
                    Ok(json) => println!("{}", json),
                    Err(e) => println!("{}", e),
                }
            } else {
                print_bonsai_changeset(&bcs);
            }
        })
        .from_err()
        .compat()
        .await
}

#[derive(Serialize)]
pub struct SerializableBonsaiChangeset {
    pub parents: Vec<ChangesetId>,
    pub author: String,
    pub author_date: DateTime,
    pub committer: Option<String>,
    // XXX should committer date always be recorded? If so, it should probably be a
    // monotonically increasing value:
    // max(author date, max(committer date of parents) + epsilon)
    pub committer_date: Option<DateTime>,
    pub message: String,
    pub extra: BTreeMap<String, Vec<u8>>,
    pub file_changes: BTreeMap<String, Option<FileChange>>,
}

impl From<BonsaiChangeset> for SerializableBonsaiChangeset {
    fn from(bonsai: BonsaiChangeset) -> Self {
        let mut parents = Vec::new();
        parents.extend(bonsai.parents());

        let author = bonsai.author().to_string();
        let author_date = bonsai.author_date().clone();

        let committer = bonsai.committer().map(|s| s.to_string());
        let committer_date = bonsai.committer_date().cloned();

        let message = bonsai.message().to_string();

        let extra = bonsai
            .extra()
            .map(|(k, v)| (k.to_string(), v.to_vec()))
            .collect();

        let file_changes = bonsai
            .file_changes()
            .map(|(k, v)| {
                (
                    String::from_utf8(k.to_vec()).expect("Found invalid UTF-8"),
                    v.cloned(),
                )
            })
            .collect();

        SerializableBonsaiChangeset {
            parents,
            author,
            author_date,
            committer,
            committer_date,
            message,
            extra,
            file_changes,
        }
    }
}
