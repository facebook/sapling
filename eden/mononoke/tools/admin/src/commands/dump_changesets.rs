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
use bulkops::Direction;
use bulkops::PublicChangesetBulkFetch;
use bytes::Bytes;
use changesets::deserialize_cs_entries;
use changesets::serialize_cs_entries;
use changesets::ChangesetEntry;
use changesets::Changesets;
use changesets::ChangesetsArc;
use changesets::ChangesetsRef;
use clap::ArgEnum;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use context::CoreContext;
use futures::future;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use itertools::Itertools;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use phases::Phases;
use phases::PhasesArc;
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
    /// Merge commits from this file into the final output. User is responsible for
    /// avoiding duplicate commits between files and database fetch. Can be repeated.
    #[clap(long)]
    merge_file: Vec<String>,
    /// What format to write to the file.
    #[clap(long, arg_enum, default_value_t=Format::Thrift)]
    output_format: Format,
    /// What format to read files with. Plaintext files can be in hg format.
    #[clap(long, arg_enum, default_value_t=Format::Thrift)]
    input_format: Format,

    #[clap(subcommand)]
    subcommand: DumpChangesetsSubcommand,
}

#[derive(Subcommand)]
pub enum DumpChangesetsSubcommand {
    /// Fetch all public changesets before dumping.
    FetchPublic(FetchPublicArgs),
    /// Don't do any extra fetching of changesets, useful for merging dumps and changing formats.
    Convert(ConvertArgs),
}

#[derive(Args)]
pub struct FetchPublicArgs {
    /// Start fetching from this commit rather than the beginning of time.
    #[clap(long)]
    start_commit: Option<String>,
    /// Start fetching from the last commit in this file, for incremental updates.
    #[clap(long)]
    start_from_file_end: Option<String>,
    /// Only look at this many commits. Notice that this may output less than LIMIT
    /// commits if there are non-public commits, but it's a good way to do this command
    /// incrementally.
    #[clap(long)]
    limit: Option<NonZeroU64>,
}

#[derive(Args)]
pub struct ConvertArgs {}

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

    async fn deserialize<'a>(
        self,
        ctx: &'a CoreContext,
        repo: &'a Repo,
        data: &'a Bytes,
    ) -> Result<Vec<ChangesetEntry>> {
        match self {
            Self::Thrift => deserialize_cs_entries(data),
            Self::Plaintext => {
                let ids: Vec<ChangesetId> = stream::iter(
                    String::from_utf8(data.iter().cloned().collect())?
                        .split_whitespace()
                        .map(|s| parse_commit_id(ctx, repo, s))
                        // prevent compiler bug
                        .collect::<Vec<_>>(),
                )
                .buffered(500)
                .try_collect()
                .await?;
                repo.changesets().get_many(ctx.clone(), ids).await
            }
        }
    }
}

impl DumpChangesetsSubcommand {
    async fn fetch_extra_changesets(
        self,
        ctx: &CoreContext,
        repo: &Repo,
        input_format: Format,
    ) -> Result<Vec<ChangesetEntry>> {
        match self {
            Self::Convert(_) => Ok(vec![]),
            Self::FetchPublic(args) => args.fetch_extra_changesets(ctx, repo, input_format).await,
        }
    }
}

impl FetchPublicArgs {
    async fn fetch_extra_changesets(
        self,
        ctx: &CoreContext,
        repo: &Repo,
        input_format: Format,
    ) -> Result<Vec<ChangesetEntry>> {
        let fetcher = PublicChangesetBulkFetch::new(repo.changesets_arc(), repo.phases_arc());

        let start_commit = {
            if let Some(path) = self.start_from_file_end {
                load_last_commit(ctx, repo, path.as_ref(), input_format).await?
            } else if let Some(start_commit) = self.start_commit {
                Some(parse_commit_id(ctx, repo, &start_commit).await?)
            } else {
                None
            }
        };

        let mut bounds = fetcher
            .get_repo_bounds_after_commits(ctx, start_commit.into_iter().collect())
            .await?;

        if let Some(limit) = self.limit {
            bounds.1 = bounds.1.min(bounds.0 + limit.get());
        }

        fetcher
            .fetch_bounded(ctx, Direction::OldestFirst, Some(bounds))
            .try_collect()
            .await
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();
    let repo: Repo = app.open_repo(&args.repo).await?;
    let input_format = args.input_format;

    let css = {
        let (mut file_css, db_css): (Vec<_>, Vec<_>) = future::try_join(
            stream::iter(
                args.merge_file
                    .iter()
                    .map(|path| load_file_contents(&ctx, &repo, path.as_ref(), input_format))
                    // prevent compiler bug
                    .collect::<Vec<_>>(),
            )
            .buffered(2)
            .try_concat(),
            args.subcommand
                .fetch_extra_changesets(&ctx, &repo, input_format),
        )
        .await?;
        file_css.extend(db_css.into_iter());
        file_css
    };

    let serialized = args.output_format.serialize(css);
    tokio::fs::write(args.out_filename, serialized).await?;

    Ok(())
}

async fn load_file_contents(
    ctx: &CoreContext,
    repo: &Repo,
    filename: &Path,
    format: Format,
) -> Result<Vec<ChangesetEntry>> {
    let file_contents = Bytes::from(tokio::fs::read(filename).await?);
    format.deserialize(ctx, repo, &file_contents).await
}

async fn load_last_commit(
    ctx: &CoreContext,
    repo: &Repo,
    filename: &Path,
    format: Format,
) -> Result<Option<ChangesetId>> {
    Ok(load_file_contents(ctx, repo, filename, format)
        .await?
        .last()
        .map(|e| e.cs_id))
}
