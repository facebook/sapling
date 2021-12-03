/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Fetch the blame of a file

use anyhow::{format_err, Context, Error};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use futures::{future, stream};
use futures_util::stream::StreamExt;
use maplit::btreeset;
use serde_json::json;
use source_control::types as thrift;
use std::fmt::Write as _;
use std::io::Write;
use std::str::FromStr;

use crate::args::commit_id::{
    add_commit_id_args, add_scheme_args, get_commit_id, get_request_schemes, get_schemes,
    map_commit_ids, resolve_commit_id,
};
use crate::args::path::{add_path_args, get_path};
use crate::args::repo::{add_repo_args, get_repo_specifier};
use crate::connection::Connection;
use crate::lib::commit_id::render_commit_id;
use crate::lib::datetime;
use crate::render::{Render, RenderStream};

pub(super) const NAME: &str = "blame";

const ARG_USER: &str = "USER";
const ARG_DATE: &str = "DATE";
const ARG_DATE_SHORT: &str = "DATE_SHORT";
const ARG_LINE_NUMBER: &str = "LINE_NUMBER";
const ARG_ORIGIN_LINE_NUMBER: &str = "ORIGIN_LINE_NUMBER";
const ARG_ORIGIN_PATH: &str = "ORIGIN_PATH";
const ARG_PARENT: &str = "PARENT";
const ARG_PARENT_LINE_RANGE: &str = "PARENT_LINE_RANGE";
const ARG_TITLE: &str = "TITLE";
const ARG_TITLE_WIDTH: &str = "TITLE_WIDTH";

const DEFAULT_TITLE_WIDTH: usize = 32;
const DEFAULT_TITLE_WIDTH_STR: &str = "32";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Fetch the blame of a file")
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_scheme_args(cmd);
    let cmd = add_commit_id_args(cmd);
    let cmd = add_path_args(cmd);
    let cmd = add_args(cmd);
    cmd
}

struct BlameOut {
    blame: thrift::Blame,
}

impl Render for BlameOut {
    fn render(&self, matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        let schemes = get_schemes(matches);
        let path = get_path(matches).expect("path is required");
        let title_width = matches
            .value_of(ARG_TITLE_WIDTH)
            .map(usize::from_str)
            .transpose()
            .context("Invalid title width")?
            .unwrap_or(DEFAULT_TITLE_WIDTH);
        let number_width = |n| ((n + 1) as f32).log10().ceil() as usize;

        match self.blame {
            thrift::Blame::blame_compact(ref blame) => {
                let max_author_len = blame.authors.iter().map(|a| a.len()).max().unwrap_or(0);
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
                            origin_path.len()
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
                            .map_or(0, |p| blame.paths[p as usize].len() + 2);
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
                    if matches.is_present(ARG_USER) {
                        write!(
                            w,
                            "{:>width$} ",
                            blame.authors[line.author_index as usize],
                            width = max_author_len as usize
                        )?;
                    }
                    render_commit_id(
                        None,
                        " ",
                        "blamed changeset",
                        &map_commit_ids(blame.commit_ids[line.commit_id_index as usize].values()),
                        &schemes,
                        w,
                    )?;
                    if matches.is_present(ARG_DATE) || matches.is_present(ARG_DATE_SHORT) {
                        let blame_date = datetime(&blame.dates[line.date_index as usize]);
                        let blame_date_formatted = if matches.is_present(ARG_DATE_SHORT) {
                            blame_date.format("%F")
                        } else {
                            blame_date.format("%+")
                        };
                        write!(w, " {}", blame_date_formatted)?;
                    }
                    if matches.is_present(ARG_TITLE) {
                        let title = match line.title_index {
                            Some(title_index) => match blame.titles.as_ref() {
                                Some(titles) => titles[title_index as usize].as_str(),
                                None => "",
                            },
                            None => "",
                        };
                        write!(w, ":{:<width$.width$}", title, width = title_width)?;
                    }
                    if matches.is_present(ARG_PARENT_LINE_RANGE) {
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
                        write!(w, ":{:>width$}", plr, width = max_parent_line_range_width)?;
                    }
                    if matches.is_present(ARG_ORIGIN_PATH) {
                        let origin_path = blame.paths[line.path_index as usize].as_str();
                        write!(
                            w,
                            ":{:>width$}",
                            if origin_path != path { origin_path } else { "" },
                            width = max_origin_path_width
                        )?;
                    }
                    if matches.is_present(ARG_ORIGIN_LINE_NUMBER) {
                        write!(
                            w,
                            ":{:>width$}",
                            line.origin_line,
                            width = max_origin_line_width
                        )?;
                    }
                    if matches.is_present(ARG_LINE_NUMBER) {
                        write!(w, ":{:>width$}", line.line, width = max_line_width)?;
                    }
                    write!(w, ": {}\n", line.contents.as_deref().unwrap_or_default())?;
                }
                Ok(())
            }
            thrift::Blame::UnknownField(id) => {
                Err(format_err!("Unknown thrift::Blame field id: {}", id))
            }
        }
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        match self.blame {
            thrift::Blame::blame_compact(ref blame) => {
                let mut lines = Vec::new();
                for line in blame.lines.iter() {
                    let mut line_json = json!({
                        "contents": line.contents.as_deref().unwrap_or_default(),
                        "commit": map_commit_ids(blame.commit_ids[line.commit_id_index as usize].values()),
                        "path": blame.paths[line.path_index as usize],
                        "line": line.line,
                        "author": blame.authors[line.author_index as usize],
                        "datetime": datetime(&blame.dates[line.date_index as usize]).to_rfc3339(),
                        "origin_line": line.origin_line,
                    });
                    let mut insert = |key, value| {
                        line_json
                            .as_object_mut()
                            .expect("line must be an object")
                            .insert(String::from(key), value);
                    };
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

pub(super) async fn run(
    matches: &ArgMatches<'_>,
    connection: Connection,
) -> Result<RenderStream, Error> {
    let repo = get_repo_specifier(matches).expect("repository is required");
    let commit_id = get_commit_id(matches)?;
    let id = resolve_commit_id(&connection, &repo, &commit_id).await?;

    let mut commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };

    if matches.is_present(ARG_PARENT) {
        let params = thrift::CommitInfoParams {
            identity_schemes: btreeset! { thrift::CommitIdentityScheme::BONSAI },
            ..Default::default()
        };
        let response = connection.commit_info(&commit, &params).await?;
        commit.id.clone_from(
            response
                .parents
                .iter()
                .next()
                .ok_or_else(|| format_err!("Commit does not have a parent"))?
                .get(&thrift::CommitIdentityScheme::BONSAI)
                .ok_or_else(|| format_err!("Could not determine ID of commit's parent"))?,
        );
    }
    let path = get_path(matches).expect("path is required");
    let commit_and_path = thrift::CommitPathSpecifier {
        commit,
        path,
        ..Default::default()
    };

    let identity_schemes = get_request_schemes(&matches);

    let params = thrift::CommitPathBlameParams {
        format: thrift::BlameFormat::COMPACT,
        identity_schemes,
        format_options: Some(btreeset! {
            thrift::BlameFormatOption::INCLUDE_CONTENTS,
            thrift::BlameFormatOption::INCLUDE_TITLE,
            thrift::BlameFormatOption::INCLUDE_PARENT,
        }),
        ..Default::default()
    };
    let response = connection
        .commit_path_blame(&commit_and_path, &params)
        .await?;
    let output: Box<dyn Render> = Box::new(BlameOut {
        blame: response.blame,
    });

    Ok(stream::once(future::ok(output)).boxed())
}

fn add_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ARG_USER)
            .short("u")
            .long("user")
            .help("List the author"),
    )
    .arg(
        Arg::with_name(ARG_DATE)
            .short("d")
            .long("date")
            .help("List the date"),
    )
    .arg(
        Arg::with_name(ARG_DATE_SHORT)
            .short("q")
            .help("List the date in short format"),
    )
    .arg(
        Arg::with_name(ARG_LINE_NUMBER)
            .short("l")
            .long("line-number")
            .help("Show current line number"),
    )
    .arg(
        Arg::with_name(ARG_ORIGIN_LINE_NUMBER)
            .short("o")
            .long("origin-line-number")
            .help("Show origin line number"),
    )
    .arg(
        Arg::with_name(ARG_ORIGIN_PATH)
            .short("O")
            .long("origin-path")
            .help("Show origin path if different from current path"),
    )
    .arg(
        Arg::with_name(ARG_PARENT)
            .long("parent")
            .help("Show blame for the first parent of the commit"),
    )
    .arg(
        Arg::with_name(ARG_PARENT_LINE_RANGE)
            .short("P")
            .long("parent-line-range")
            .help("Show the line range in the parent this line replaces"),
    )
    .arg(
        Arg::with_name(ARG_TITLE)
            .short("T")
            .long("title")
            .help("Show the title (first line of the commit message) of the blamed changeset"),
    )
    .arg(
        Arg::with_name(ARG_TITLE_WIDTH)
            .long("title-width")
            .help("Set the maximum width of the title (if shown)")
            .takes_value(true)
            .default_value(DEFAULT_TITLE_WIDTH_STR),
    )
}
