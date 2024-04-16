/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! List the contents of a directory.

use std::io::Write;

use anyhow::bail;
use anyhow::Result;
use cloned::cloned;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::Stream;
use serde::Serialize;
use source_control as thrift;

use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::repo::RepoArgs;
use crate::connection::Connection;
use crate::library::summary::run_stress;
use crate::library::summary::summary_output;
use crate::library::summary::StressArgs;
use crate::render::Render;
use crate::util::byte_count_short;
use crate::ScscApp;

const CHUNK_SIZE: i64 = source_control::TREE_LIST_MAX_LIMIT;

/// Number of concurrent fetches for very large directories.
const CONCURRENT_FETCHES: usize = 10;

/// Number of concurrent fetches for item info (e.g. symlink target).
const CONCURRENT_ITEM_FETCHES: usize = 100;

const MAX_LINK_NAME_LEN: i64 = 4096;

#[derive(clap::Parser)]
/// List the contents of a directory
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(long, short, default_value = "")]
    /// Path to list
    path: String,
    #[clap(long, short)]
    /// Show hidden files (starting with '.')
    all: bool,
    #[clap(long, short)]
    /// Show additional information for each entry
    long: bool,
    /// Enable stress test mode
    #[clap(flatten)]
    stress: Option<StressArgs>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum LsEntryOutput {
    Tree {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        simple_format_sha1: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        simple_format_sha256: Option<String>,
        child_files_count: i64,
        child_files_total_size: i64,
        child_dirs_count: i64,
        descendant_files_count: i64,
        descendant_files_total_size: i64,
    },
    File {
        id: String,
        size: i64,
        content_sha1: String,
        content_sha256: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        link_target: Option<String>,
    },
}

#[derive(Serialize)]
struct LsOutput {
    name: String,
    r#type: String,
    #[serde(flatten)]
    entry: LsEntryOutput,
}

impl LsOutput {
    fn render_short(&self, w: &mut dyn Write) -> Result<()> {
        match self.entry {
            LsEntryOutput::Tree { .. } => write!(w, "{}/\n", self.name)?,
            LsEntryOutput::File { .. } => write!(w, "{}\n", self.name)?,
        }
        Ok(())
    }

    fn render_long(&self, w: &mut dyn Write) -> Result<()> {
        match &self.entry {
            LsEntryOutput::Tree {
                descendant_files_total_size,
                ..
            } => {
                write!(
                    w,
                    "{}  {:>8}  {}/\n",
                    self.r#type.to_string().to_lowercase(),
                    byte_count_short(*descendant_files_total_size),
                    self.name
                )?;
            }
            LsEntryOutput::File {
                size,
                link_target: Some(link_target),
                ..
            } => {
                write!(
                    w,
                    "{}  {:>8}  {} -> {}\n",
                    self.r#type.to_string().to_lowercase(),
                    byte_count_short(*size),
                    self.name,
                    link_target,
                )?;
            }
            LsEntryOutput::File { size, .. } => {
                write!(
                    w,
                    "{}  {:>8}  {}\n",
                    self.r#type.to_string().to_lowercase(),
                    byte_count_short(*size),
                    self.name
                )?;
            }
        }
        Ok(())
    }
}

impl Render for LsOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if args.all || !self.name.starts_with('.') {
            if args.long {
                self.render_long(w)?;
            } else {
                self.render_short(w)?;
            }
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

async fn fetch_link_target(
    connection: Connection,
    repo: thrift::RepoSpecifier,
    id: Vec<u8>,
) -> Option<String> {
    let file = thrift::FileSpecifier::by_id(thrift::FileIdSpecifier {
        repo,
        id,
        ..Default::default()
    });
    let params = thrift::FileContentChunkParams {
        offset: 0,
        size: MAX_LINK_NAME_LEN,
        ..Default::default()
    };
    match connection.file_content_chunk(&file, &params).await {
        Ok(response) => Some(String::from_utf8_lossy(&response.data).into_owned()),
        Err(_) => None,
    }
}

fn list_output(
    connection: Connection,
    repo: thrift::RepoSpecifier,
    response: thrift::TreeListResponse,
    long: bool,
) -> impl Stream<Item = Result<LsOutput>> {
    stream::iter(response.entries)
        .map(move |entry| {
            let connection = connection.clone();
            let repo = repo.clone();
            async move {
                let entry_output = match entry.info {
                    thrift::EntryInfo::tree(info) => {
                        let id = faster_hex::hex_string(&info.id);
                        let simple_format_sha1 = info
                            .simple_format_sha1
                            .as_deref()
                            .map(faster_hex::hex_string);
                        let simple_format_sha256 = info
                            .simple_format_sha256
                            .as_deref()
                            .map(faster_hex::hex_string);
                        LsEntryOutput::Tree {
                            id,
                            simple_format_sha1,
                            simple_format_sha256,
                            child_files_count: info.child_files_count,
                            child_dirs_count: info.child_dirs_count,
                            child_files_total_size: info.child_files_total_size,
                            descendant_files_count: info.descendant_files_count,
                            descendant_files_total_size: info.descendant_files_total_size,
                        }
                    }
                    thrift::EntryInfo::file(info) => {
                        let id = faster_hex::hex_string(&info.id);
                        let content_sha1 = faster_hex::hex_string(&info.content_sha1);
                        let content_sha256 = faster_hex::hex_string(&info.content_sha256);
                        let link_target = if long && entry.r#type == thrift::EntryType::LINK {
                            fetch_link_target(connection.clone(), repo.clone(), info.id.clone())
                                .await
                        } else {
                            None
                        };
                        LsEntryOutput::File {
                            id,
                            content_sha1,
                            content_sha256,
                            size: info.file_size,
                            link_target,
                        }
                    }
                    _ => {
                        bail!("malformed response format for '{}'", entry.name);
                    }
                };
                let r#type = match entry.r#type {
                    thrift::EntryType::GIT_SUBMODULE => "gitm".to_string(),
                    value => value.to_string().to_lowercase(),
                };
                Ok(LsOutput {
                    name: entry.name,
                    r#type,
                    entry: entry_output,
                })
            }
        })
        .buffered(CONCURRENT_ITEM_FETCHES)
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let conn = app.get_connection(Some(&repo.name))?;
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    };
    let path = args.path.clone();
    let tree = thrift::TreeSpecifier::by_commit_path(thrift::CommitPathSpecifier {
        commit,
        path,
        ..Default::default()
    });

    // Request the first chunk of the directory listing.
    let params = thrift::TreeListParams {
        offset: 0,
        limit: CHUNK_SIZE,
        ..Default::default()
    };
    if let Some(stress) = args.stress {
        let results = run_stress(stress.count, stress.parallel, || {
            cloned!(conn, params, tree);
            Box::pin(async move {
                conn.tree_list(&tree, &params).await?;
                Ok(())
            })
        })
        .await;

        let output = summary_output(results);
        return app.target.render(&(), output).await;
    }

    let response = conn.tree_list(&tree, &params).await?;
    let count = response.count;
    let long = args.long;
    let output = list_output(conn.clone(), repo.clone(), response, long).chain(
        stream::iter((CHUNK_SIZE..count).step_by(CHUNK_SIZE as usize))
            .map({
                let connection = conn.clone();
                move |offset| {
                    // Request subsequent chunks of the directory listing.
                    let params = thrift::TreeListParams {
                        offset,
                        limit: CHUNK_SIZE,
                        ..Default::default()
                    };
                    connection.tree_list(&tree, &params)
                }
            })
            .buffered(CONCURRENT_FETCHES)
            .then(move |response| {
                let repo = repo.clone();
                let connection = conn.clone();
                async move {
                    response.map(move |response| {
                        list_output(connection.clone(), repo.clone(), response, long)
                    })
                }
            })
            .try_flatten(),
    );
    app.target.render(&args, output).await
}
