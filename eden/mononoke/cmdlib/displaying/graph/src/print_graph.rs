/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::default::Default;
use std::io::Write;

use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcherArc;
use cmdlib_displaying::display_file_change;
use context::CoreContext;
use dag::render::Ancestor;
use dag::render::GraphRowRenderer;
use dag::render::Renderer;
use futures::future::join_all;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::Generation;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreArc;

pub trait Repo = ChangesetFetcherArc
    + RepoBlobstoreArc
    + BonsaiHgMappingRef
    + BonsaiGitMappingRef
    + BonsaiGlobalrevMappingRef;

#[derive(Debug)]
pub struct PrintGraphOptions {
    /// Maximum distance from the initial changesets to any displayed changeset.
    /// Defaults to 10.
    pub limit: usize,

    /// Display commit message for all displayed changesets
    pub display_message: bool,

    /// Display bonsai id for all displayed changesets
    pub display_id: bool,

    /// Display commit author for all displayed changesets
    pub display_author: bool,

    /// Display commit author date for all displayed changesets
    pub display_author_date: bool,

    /// Display commit's file changes
    pub display_file_changes: bool,
}

impl Default for PrintGraphOptions {
    fn default() -> Self {
        PrintGraphOptions {
            limit: 10,
            display_message: Default::default(),
            display_id: Default::default(),
            display_author: Default::default(),
            display_author_date: Default::default(),
            display_file_changes: Default::default(),
        }
    }
}

pub fn get_message(opts: &PrintGraphOptions, cs: BonsaiChangeset) -> String {
    let message_vec = {
        let mut message_vec = Vec::new();

        if opts.display_message {
            message_vec.push(format!("message: {}", cs.message()));
        }

        if opts.display_id {
            message_vec.push(format!("id: {}", cs.get_changeset_id()));
        }

        if opts.display_author {
            message_vec.push(format!("author: {}", cs.author()));
        }

        if opts.display_author_date {
            message_vec.push(format!("author date: {}", cs.author_date()));
        }
        message_vec
    };

    let file_changes_msg = {
        let mut file_changes_msg = Vec::new();

        if opts.display_file_changes {
            file_changes_msg.push(String::from(" File changes:"));
            let file_changes = cs
                .file_changes()
                .collect::<Vec<(&NonRootMPath, &FileChange)>>();

            for (path, change) in file_changes.iter() {
                file_changes_msg.push(display_file_change(&path.to_string(), change));
            }
        }
        file_changes_msg.push(String::from("\n"));
        file_changes_msg
    };

    format!(
        "{}\n{}",
        message_vec.join(", "),
        file_changes_msg.join("\n")
    )
}

pub async fn graph_changesets<W>(
    ctx: &CoreContext,
    repo: &impl Repo,
    opts: PrintGraphOptions,
    changeset_fetcher: ArcChangesetFetcher,
    changesets: Vec<ChangesetId>,
    mut writer: Box<W>,
) -> Result<()>
where
    W: Write + Send,
{
    let blobstore = repo.repo_blobstore();

    let mut minimum_distance: HashMap<ChangesetId, usize> =
        changesets.iter().cloned().map(|id| (id, 0)).collect();

    let mut to_visit: BinaryHeap<(Generation, ChangesetId)> =
        join_all(changesets.into_iter().map(|head| {
            let changeset_fetcher = &changeset_fetcher;
            async move {
                Ok((
                    changeset_fetcher.get_generation_number(ctx, head).await?,
                    head,
                ))
            }
        }))
        .await
        .into_iter()
        .collect::<Result<_>>()?;

    let mut renderer = GraphRowRenderer::<ChangesetId>::new()
        .output()
        .build_box_drawing();

    while let Some((_, hash)) = to_visit.pop() {
        let parents = changeset_fetcher.get_parents(ctx, hash).await?;
        let current_distance = *minimum_distance.get(&hash).unwrap();

        if current_distance > opts.limit {
            writer.write_fmt(format_args!(
                "{}\n",
                renderer
                    .next_row(hash, Vec::new(), String::from("~"), String::from(""))
                    .trim_end(),
            ))?;
            continue;
        }

        let cs = hash
            .load(ctx, blobstore)
            .await
            .with_context(|| format!("Failed to load changeset {}", hash))?;

        writer.write_fmt(format_args!(
            "{}\n",
            renderer
                .next_row(
                    hash,
                    parents.iter().cloned().map(Ancestor::Parent).collect(),
                    String::from("o"),
                    get_message(&opts, cs),
                )
                .trim_end(),
        ))?;

        for parent_id in parents.into_iter() {
            if let Some(&distance) = minimum_distance.get(&parent_id) {
                if current_distance + 1 < distance {
                    minimum_distance.insert(parent_id, current_distance + 1);
                }
            } else {
                let parent_generation = changeset_fetcher
                    .get_generation_number(ctx, parent_id)
                    .await?;

                minimum_distance.insert(parent_id, current_distance + 1);
                to_visit.push((parent_generation, parent_id));
            }
        }
    }

    Ok(())
}

pub async fn print_graph<W>(
    ctx: &CoreContext,
    repo: &impl Repo,
    // Initial changesets to start displaying from
    mut changesets: Vec<ChangesetId>,
    opts: PrintGraphOptions,
    writer: Box<W>,
) -> Result<()>
where
    W: Write + Send,
{
    let changeset_fetcher = repo.changeset_fetcher_arc();

    changesets.sort();
    changesets.dedup();

    graph_changesets(ctx, repo, opts, changeset_fetcher, changesets, writer).await
}
