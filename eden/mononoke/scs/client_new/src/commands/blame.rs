/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Fetch the blame of a file

use anyhow::format_err;
use anyhow::Result;
use clap::Parser;
use maplit::btreeset;
use serde_json::json;
use source_control::types as thrift;
use std::fmt::Write as _;
use std::io::Write;
use unicode_truncate::Alignment;
use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::path::PathArgs;
use crate::args::repo::RepoArgs;
use crate::lib::commit_id::render_commit_id;
use crate::lib::datetime;
use crate::render::Render;
use crate::ScscApp;

const DEFAULT_TITLE_WIDTH: usize = 32;

#[derive(Parser)]
/// Fetch the blame of a file
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(flatten)]
    path_args: PathArgs,

    #[clap(long)]
    /// Show blame for the first parent of the commit
    parent: bool,
    #[clap(long, conflicts_with = "parent")]
    /// Show blame for the Nth parent of the commit
    parent_index: Option<usize>,
    #[clap(long, short)]
    /// List the author
    user: bool,
    #[clap(long, short)]
    /// List the date
    date: bool,
    #[clap(short = 'q')]
    /// List the date in short format
    date_short: bool,
    #[clap(long, short)]
    /// Show current line number
    line_number: bool,
    #[clap(long, short)]
    /// Show origin line number
    origin_line_number: bool,
    #[clap(long, short = 'O')]
    /// Show origin path if different from current path
    origin_path: bool,
    #[clap(long, short = 'P')]
    /// Show the line range in the parent this line replaces
    parent_line_range: bool,
    #[clap(long, short = 'T')]
    /// Show the title (first line of the commit message) of the blamed changeset
    title: bool,
    #[clap(long, default_value_t = DEFAULT_TITLE_WIDTH)]
    /// Set thc maxiucbdrkccuefmum width of the title (if shown)
    title_width: usize,
    #[clap(long, short = 'n')]
    /// Show numbers for commits (specific to this blame revision)
    commit_number: bool,
    #[clap(long)]
    /// Do not show commit ids
    no_commit_id: bool,
}

struct BlameOut {
    blame: thrift::Blame,
}

impl Render for BlameOut {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        let schemes = args.scheme_args.scheme_string_set();
        let path = args.path_args.path.clone();
        let title_width = args.title_width;
        let number_width = |n| ((n + 1) as f32).log10().ceil() as usize;

        match self.blame {
            thrift::Blame::blame_compact(ref blame) => {
                let max_commit_number_width = number_width(
                    blame
                        .commit_numbers
                        .iter()
                        .flatten()
                        .copied()
                        .max()
                        .unwrap_or(0),
                );
                let max_author_width = blame.authors.iter().map(|a| a.width()).max().unwrap_or(0);
                let max_line_width =
                    number_width(blame.lines.iter().map(|l| l.line).max().unwrap_or(0) + 1);

                let max_origin_line_width =
                    number_width(blame.lines.iter().map(|l| l.origin_line).max().unwrap_or(0));
                let max_origin_path_width = blame
                    .lines
                    .iter()
                    .map(|l| {
                        let origin_path = blame.paths[l.path_index as usize].as_str();
                        if origin_path != path {
                            origin_path.width()
                        } else {
                            0
                        }
                    })
                    .max()
                    .unwrap_or(0);
                let max_parent_line_range_width = blame
                    .lines
                    .iter()
                    .map(|l| {
                        let parent_index_width = l
                            .parent_index
                            .map_or(0, |i| if i > 0 { number_width(i) + 2 } else { 0 });
                        let parent_path_width = l
                            .parent_path_index
                            .map_or(0, |p| blame.paths[p as usize].width() + 2);
                        let parent_range_width = match (l.parent_start_line, l.parent_range_length)
                        {
                            (Some(start), Some(0)) => number_width(start) + 1,
                            (Some(start), Some(len)) => {
                                number_width(start) + number_width(start + len - 1) + 1
                            }
                            _ => 0,
                        };
                        parent_index_width + parent_path_width + parent_range_width
                    })
                    .max()
                    .unwrap_or(0);

                for line in blame.lines.iter() {
                    let mut separator = "";
                    if args.user {
                        let author = blame.authors[line.author_index as usize].as_str();
                        write!(
                            w,
                            "{}",
                            author.unicode_pad(max_author_width, Alignment::Right, false),
                        )?;
                        separator = " ";
                    }
                    if args.commit_number {
                        if let Some(commit_numbers) = &blame.commit_numbers {
                            let commit_number =
                                format!("#{}", commit_numbers[line.commit_id_index as usize]);
                            write!(
                                w,
                                "{}{:>width$}",
                                separator,
                                commit_number,
                                width = max_commit_number_width + 1
                            )?
                        }
                        separator = " ";
                    }
                    if !args.no_commit_id {
                        write!(w, "{}", separator)?;
                        render_commit_id(
                            None,
                            " ",
                            "blamed changeset",
                            &map_commit_ids(
                                blame.commit_ids[line.commit_id_index as usize].values(),
                            ),
                            &schemes,
                            w,
                        )?;
                        separator = " ";
                    }
                    if args.date || args.date_short {
                        let blame_date = datetime(&blame.dates[line.date_index as usize]);
                        let blame_date_formatted = if args.date_short {
                            blame_date.format("%F")
                        } else {
                            blame_date.format("%+")
                        };
                        write!(w, "{}{}", separator, blame_date_formatted)?;
                        separator = " ";
                    }
                    if !separator.is_empty() {
                        separator = ":";
                    }
                    if args.title {
                        let title = match line.title_index {
                            Some(title_index) => match blame.titles.as_ref() {
                                Some(titles) => titles[title_index as usize].as_str(),
                                None => "",
                            },
                            None => "",
                        };
                        write!(
                            w,
                            "{}{}",
                            separator,
                            title.unicode_pad(title_width, Alignment::Left, true)
                        )?;
                        separator = ":";
                    }
                    if args.parent_line_range {
                        let mut plr = String::with_capacity(max_parent_line_range_width);
                        if let Some(parent_index) = line.parent_index {
                            if parent_index != 0 {
                                write!(plr, "({})", parent_index)?;
                            }
                        }
                        if let Some(path_index) = line.parent_path_index {
                            write!(plr, "[{}]", blame.paths[path_index as usize])?;
                        }
                        if let (Some(start), Some(length)) =
                            (line.parent_start_line, line.parent_range_length)
                        {
                            if length == 0 {
                                write!(plr, "+{}", start)?;
                            } else {
                                write!(plr, "{}-{}", start, start + length - 1)?;
                            }
                        }
                        write!(
                            w,
                            "{}{}",
                            separator,
                            plr.unicode_pad(max_parent_line_range_width, Alignment::Right, false)
                        )?;
                        separator = ":";
                    }
                    if args.origin_path {
                        let origin_path = blame.paths[line.path_index as usize].as_str();
                        write!(
                            w,
                            "{}{}",
                            separator,
                            if origin_path != path { origin_path } else { "" }.unicode_pad(
                                max_origin_path_width,
                                Alignment::Right,
                                false
                            )
                        )?;
                        separator = ":";
                    }
                    if args.origin_line_number {
                        write!(
                            w,
                            "{}{:>width$}",
                            separator,
                            line.origin_line,
                            width = max_origin_line_width
                        )?;
                        separator = ":";
                    }
                    if args.line_number {
                        write!(
                            w,
                            "{}{:>width$}",
                            separator,
                            line.line,
                            width = max_line_width
                        )?;
                        separator = ":";
                    }
                    if !separator.is_empty() {
                        separator = ": ";
                    }
                    write!(
                        w,
                        "{}{}\n",
                        separator,
                        line.contents.as_deref().unwrap_or_default()
                    )?;
                }
                Ok(())
            }
            thrift::Blame::UnknownField(id) => {
                Err(format_err!("Unknown thrift::Blame field id: {}", id))
            }
        }
    }

    fn render_json(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        match self.blame {
            thrift::Blame::blame_compact(ref blame) => {
                let mut lines = Vec::new();
                let use_short_date = args.date_short;
                for line in blame.lines.iter() {
                    let blame_date = datetime(&blame.dates[line.date_index as usize]);
                    let formatted_blame_date = if use_short_date {
                        blame_date.format("%F")
                    } else {
                        blame_date.format("%+")
                    };
                    let mut line_json = json!({
                        "contents": line.contents.as_deref().unwrap_or_default(),
                        "commit": map_commit_ids(blame.commit_ids[line.commit_id_index as usize].values()),
                        "path": blame.paths[line.path_index as usize],
                        "line": line.line,
                        "author": blame.authors[line.author_index as usize],
                        "datetime": formatted_blame_date.to_string(),
                        "origin_line": line.origin_line,
                    });
                    let mut insert = |key, value| {
                        line_json
                            .as_object_mut()
                            .expect("line must be an object")
                            .insert(String::from(key), value);
                    };
                    if let Some(commit_numbers) = &blame.commit_numbers {
                        insert(
                            "commit_number",
                            commit_numbers[line.commit_id_index as usize].into(),
                        );
                    }
                    if let (Some(title_index), Some(titles)) =
                        (line.title_index, blame.titles.as_ref())
                    {
                        insert("title", titles[title_index as usize].clone().into());
                    }
                    if let Some(parent_index) = line.parent_index {
                        insert("parent_index", parent_index.into());
                        if let Some(parent_commit_ids) = &blame.parent_commit_ids {
                            let parents = &parent_commit_ids[line.commit_id_index as usize];
                            insert(
                                "parent",
                                json!(map_commit_ids(parents[parent_index as usize].values())),
                            );
                        }
                    }
                    if let Some(path_index) = line.parent_path_index {
                        insert(
                            "parent_path",
                            blame.paths[path_index as usize].clone().into(),
                        );
                    }
                    if let (Some(start), Some(length)) =
                        (line.parent_start_line, line.parent_range_length)
                    {
                        insert("parent_start_line", start.into());
                        insert("parent_range_length", length.into());
                    }
                    lines.push(line_json);
                }
                Ok(serde_json::to_writer(w, &lines)?)
            }
            thrift::Blame::UnknownField(id) => {
                Err(format_err!("Unknown thrift::Blame field id: {}", id))
            }
        }
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let id = resolve_commit_id(&app.connection, &repo, &commit_id).await?;

    let mut commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };

    let parent_index = if args.parent {
        Some(0)
    } else {
        args.parent_index
    };

    if let Some(parent_index) = parent_index {
        let params = thrift::CommitInfoParams {
            identity_schemes: btreeset! { thrift::CommitIdentityScheme::BONSAI },
            ..Default::default()
        };
        let response = app.connection.commit_info(&commit, &params).await?;
        commit.id.clone_from(
            response
                .parents
                .get(parent_index)
                .ok_or_else(|| {
                    format_err!("Commit does not have a parent with index {}", parent_index)
                })?
                .get(&thrift::CommitIdentityScheme::BONSAI)
                .ok_or_else(|| {
                    format_err!(
                        "Could not determine ID of commit's parent with index {}",
                        parent_index
                    )
                })?,
        );
    }
    let path = args.path_args.path.clone();
    let commit_and_path = thrift::CommitPathSpecifier {
        commit,
        path,
        ..Default::default()
    };

    let identity_schemes = args.scheme_args.clone().into_request_schemes();

    let params = thrift::CommitPathBlameParams {
        format: thrift::BlameFormat::COMPACT,
        identity_schemes,
        format_options: Some(btreeset! {
            thrift::BlameFormatOption::INCLUDE_CONTENTS,
            thrift::BlameFormatOption::INCLUDE_TITLE,
            thrift::BlameFormatOption::INCLUDE_PARENT,
            thrift::BlameFormatOption::INCLUDE_COMMIT_NUMBERS,
        }),
        ..Default::default()
    };
    let response = app
        .connection
        .commit_path_blame(&commit_and_path, &params)
        .await?;
    app.target
        .render_one(
            &args,
            BlameOut {
                blame: response.blame,
            },
        )
        .await
}
