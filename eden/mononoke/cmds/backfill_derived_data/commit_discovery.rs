/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobrepo::BlobRepo;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use cmdlib::helpers;
use context::CoreContext;
use mononoke_types::ChangesetId;

const ARG_CHANGESET: &str = "changeset";
const ARG_INPUT_FILE: &str = "input-file";

pub enum CommitDiscoveryOptions {
    Changesets(Vec<ChangesetId>),
}

impl CommitDiscoveryOptions {
    pub fn add_opts<'a, 'b>(subcommand: App<'a, 'b>) -> App<'a, 'b> {
        subcommand
            .arg(
                Arg::with_name(ARG_CHANGESET)
                    .long(ARG_CHANGESET)
                    .takes_value(true)
                    .required(false)
                    .conflicts_with(ARG_INPUT_FILE)
                    .help("changeset by {hg|bonsai} hash or bookmark"),
            )
            .arg(
                Arg::with_name(ARG_INPUT_FILE)
                    .long(ARG_INPUT_FILE)
                    .takes_value(true)
                    .required(false)
                    .help("File with a list of changeset hashes {hd|bonsai} or bookmarks"),
            )
    }

    pub async fn from_matches(
        ctx: &CoreContext,
        repo: &BlobRepo,
        matches: &ArgMatches<'_>,
    ) -> Result<CommitDiscoveryOptions, Error> {
        if let Some(hash_or_bookmark) = matches.value_of(ARG_CHANGESET) {
            let csid = helpers::csid_resolve(ctx, repo.clone(), hash_or_bookmark).await?;
            return Ok(CommitDiscoveryOptions::Changesets(vec![csid]));
        }

        if let Some(input_file) = matches.value_of(ARG_INPUT_FILE) {
            let csids = helpers::csids_resolve_from_file(ctx, repo, input_file).await?;
            return Ok(CommitDiscoveryOptions::Changesets(csids));
        }

        Err(anyhow!("No commits are specified"))
    }

    pub fn get_commits(self) -> Vec<ChangesetId> {
        match self {
            CommitDiscoveryOptions::Changesets(csids) => csids,
        }
    }
}
