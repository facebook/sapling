/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::Context;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use clap_old::App;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStore;
use fbinit::FacebookInit;
use mononoke_types::hash::GitSha1;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;
use serde_derive::Serialize;
use slog::Logger;

use crate::common::fetch_bonsai_changeset;
use crate::common::print_bonsai_changeset;
use crate::error::SubcommandError;

pub const BONSAI_FETCH: &str = "bonsai-fetch";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(BONSAI_FETCH)
        .about("fetches bonsai changeset information")
        .args_from_usage(
            r#"<CHANGESET_ID>    'bonsai id to fetch'
                          -b --bubble [ID]       'if provided, will check bubble'
                          --json            'if provided json will be returned'"#,
        )
}

pub async fn subcommand_bonsai_fetch<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let rev = sub_m.value_of("CHANGESET_ID").unwrap().to_string();

    let ctx = CoreContext::new_with_logger_and_client_info(
        fb,
        logger.clone(),
        ClientInfo::default_with_entry_point(ClientEntryPoint::MononokeAdmin),
    );
    let json_flag = sub_m.is_present("json");
    let bubble = sub_m
        .value_of("bubble")
        .map(std::num::NonZeroU64::from_str)
        .transpose()
        .context("parsing --bubble")?
        .map(BubbleId::new);

    #[facet::container]
    struct BonsaiFetchContainer {
        #[facet]
        id: RepoIdentity,
        #[facet]
        hg_mapping: dyn BonsaiHgMapping,
        #[facet]
        bookmarks: dyn Bookmarks,
        #[facet]
        blobstore: RepoBlobstore,
        #[facet]
        ephemeral_blobstore: RepoEphemeralStore,
    }
    let container: BonsaiFetchContainer =
        args::not_shardmanager_compatible::open_repo(fb, &logger, matches).await?;
    let blobstore = if let Some(bubble_id) = bubble {
        let bubble = container.ephemeral_blobstore.open_bubble(bubble_id).await?;
        bubble.wrap_repo_blobstore((*container.blobstore).clone())
    } else {
        (*container.blobstore).clone()
    };
    let bcs = fetch_bonsai_changeset(ctx, &rev, container, &blobstore).await?;
    if json_flag {
        match serde_json::to_string(&SerializableBonsaiChangeset::from(bcs)) {
            Ok(json) => println!("{}", json),
            Err(e) => println!("{}", e),
        }
    } else {
        print_bonsai_changeset(&bcs);
    }
    Ok(())
}

#[derive(Serialize)]
pub struct SerializableBonsaiChangeset {
    pub parents: Vec<ChangesetId>,
    pub author: String,
    pub author_date: DateTime,
    pub committer: Option<String>,
    pub committer_date: Option<DateTime>,
    pub message: String,
    pub hg_extra: BTreeMap<String, Vec<u8>>,
    pub git_extra_headers: Option<BTreeMap<Vec<u8>, Vec<u8>>>,
    pub file_changes: BTreeMap<String, FileChange>,
    // SHA1 hash representing a git tree object. If this changeset
    // corresponds to a Git tree object, then this field will have
    // value, otherwise it would be omitted.
    pub git_tree_hash: Option<GitSha1>,
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

        let hg_extra = bonsai
            .hg_extra()
            .map(|(k, v)| (k.to_string(), v.to_vec()))
            .collect();

        let git_extra_headers = bonsai
            .git_extra_headers()
            .map(|extra| extra.map(|(k, v)| (k.to_vec(), v.to_vec())).collect());
        let file_changes = bonsai
            .file_changes_map()
            .iter()
            .map(|(k, v)| {
                (
                    String::from_utf8(k.to_vec()).expect("Found invalid UTF-8"),
                    v.clone(),
                )
            })
            .collect();
        let git_tree_hash = bonsai.git_tree_hash().cloned();

        SerializableBonsaiChangeset {
            parents,
            author,
            author_date,
            committer,
            committer_date,
            message,
            hg_extra,
            git_extra_headers,
            file_changes,
            git_tree_hash,
        }
    }
}
