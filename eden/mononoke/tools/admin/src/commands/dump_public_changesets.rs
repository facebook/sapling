/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bulkops::{Direction, PublicChangesetBulkFetch};
use bytes::Bytes;
use changesets::{
    deserialize_cs_entries, serialize_cs_entries, ChangesetEntry, Changesets, ChangesetsArc,
};
use clap::{ArgEnum, Parser};
use futures::{future, stream, StreamExt, TryStreamExt};
use itertools::Itertools;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use phases::{Phases, PhasesArc};
use std::num::NonZeroU64;
use std::path::Path;

use crate::commit_id::parse_commit_id;

#[derive(Debug, Clone, Copy, ArgEnum)]
enum Format {
    /// Thrift serialized ChangesetEntry, with info about repo, parents and generation number.
    Thrift,
    /// One plaintext bonsai id per line, without any repo information
    Plaintext,
}

/// Dump all public changeset entries to a file.
#[derive(Parser)]
pub struct CommandArgs {
    /// Which repo to dump changesets from.
    #[clap(flatten)]
    repo: RepoArgs,

    /// File name where commits will be saved.
    #[clap(long)]
    out_filename: String,
    /// Start fetching from this commit rather than the beginning of time.
    #[clap(long)]
    start_commit: Option<String>,
    /// Start fetching from the last commit in this file, for incremental updates.
    #[clap(long)]
    start_from_file_end: Option<String>,
    /// Merge commits from this file into the final output. User is responsible for
    /// avoiding duplicate commits between files and database fetch. Can be repeated.
    #[clap(long)]
    merge_file: Vec<String>,
    /// Only look at this many commits. Notice that this may output less than LIMIT
    /// commits if there are non-public commits, but it's a good way to do this command
    /// incrementally.
    #[clap(long)]
    limit: Option<NonZeroU64>,
    /// What format to write to the file.
    #[clap(long, arg_enum, default_value_t=Format::Thrift)]
    output_format: Format,
}

#[facet::container]
pub struct Repo {
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    changesets: dyn Changesets,

    #[facet]
    phases: dyn Phases,
}

impl Format {
    fn serialize(self, entries: Vec<ChangesetEntry>) -> Bytes {
        match self {
            Self::Thrift => serialize_cs_entries(entries),
            Self::Plaintext => Bytes::from(entries.into_iter().map(|e| e.cs_id).join("\n")),
        }
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();
    let repo: Repo = app.open_repo(&args.repo).await?;

    let fetcher = PublicChangesetBulkFetch::new(repo.changesets_arc(), repo.phases_arc());

    let start_commit = {
        if let Some(path) = args.start_from_file_end {
            load_last_commit(path.as_ref()).await?
        } else if let Some(start_commit) = args.start_commit {
            Some(parse_commit_id(&ctx, &repo, &start_commit).await?)
        } else {
            None
        }
    };

    let mut bounds = fetcher
        .get_repo_bounds_after_commits(&ctx, start_commit.into_iter().collect())
        .await?;
    if let Some(limit) = args.limit {
        bounds.1 = bounds.1.min(bounds.0 + limit.get());
    }

    let css = {
        let (mut file_css, db_css): (Vec<_>, Vec<_>) = future::try_join(
            stream::iter(
                args.merge_file
                    .iter()
                    .map(|path| load_file_contents(path.as_ref()))
                    // prevent compiler bug
                    .collect::<Vec<_>>(),
            )
            .buffered(2)
            .try_concat(),
            fetcher
                .fetch_bounded(&ctx, Direction::OldestFirst, Some(bounds))
                .try_collect::<Vec<_>>(),
        )
        .await?;
        file_css.extend(db_css.into_iter());
        file_css
    };

    let serialized = args.output_format.serialize(css);
    tokio::fs::write(args.out_filename, serialized).await?;

    Ok(())
}

async fn load_file_contents(filename: &Path) -> Result<Vec<ChangesetEntry>> {
    let file_contents = Bytes::from(tokio::fs::read(filename).await?);
    deserialize_cs_entries(&file_contents)
}

async fn load_last_commit(filename: &Path) -> Result<Option<ChangesetId>> {
    Ok(load_file_contents(filename).await?.last().map(|e| e.cs_id))
}
