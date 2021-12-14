/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Display information about a commit, directory, or file.

use std::collections::HashSet;
use std::io::Write;

use anyhow::{bail, Error};
use clap::{App, AppSettings, ArgMatches, SubCommand};
use futures::stream;
use futures_util::stream::StreamExt;
use serde_derive::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::{
    add_commit_id_args, add_scheme_args, get_commit_id, get_request_schemes, get_schemes,
    resolve_commit_id,
};
use crate::args::path::{add_optional_multiple_path_args, get_paths};
use crate::args::repo::{add_repo_args, get_repo_specifier};
use crate::connection::Connection;
use crate::lib::commit::{render_commit_info, CommitInfo};
use crate::render::{Render, RenderStream};
use crate::util::{byte_count_iec, plural};

pub(super) const NAME: &str = "info";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Fetch info about a commit, directory or file")
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_scheme_args(cmd);
    let cmd = add_commit_id_args(cmd);
    let cmd = add_optional_multiple_path_args(cmd);
    cmd
}

struct CommitInfoOutput {
    commit: CommitInfo,
    requested: String,
    schemes: HashSet<String>,
}

impl Render for CommitInfoOutput {
    fn render(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        render_commit_info(&self.commit, &self.requested, &self.schemes, w)
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        Ok(serde_json::to_writer(w, &self.commit)?)
    }
}

#[derive(Serialize)]
struct TreeInfoOutput {
    path: String,
    r#type: String,
    id: String,
    simple_format_sha1: String,
    simple_format_sha256: String,
    child_files_count: i64,
    child_files_total_size: i64,
    child_dirs_count: i64,
    descendant_files_count: i64,
    descendant_files_total_size: i64,
}

impl Render for TreeInfoOutput {
    fn render(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        write!(w, "Path: {}\n", self.path)?;
        write!(w, "Type: {}\n", self.r#type)?;
        write!(w, "Id: {}\n", self.id)?;
        write!(w, "Simple-Format-SHA1: {}\n", self.simple_format_sha1)?;
        write!(w, "Simple-Format-SHA256: {}\n", self.simple_format_sha256)?;
        write!(
            w,
            "Children: {} {} ({}), {} {}\n",
            self.child_files_count,
            plural(self.child_files_count, "file", "files"),
            byte_count_iec(self.child_files_total_size),
            self.child_dirs_count,
            plural(self.child_dirs_count, "dir", "dirs"),
        )?;
        write!(
            w,
            "Descendants: {} {} ({})\n",
            self.descendant_files_count,
            plural(self.descendant_files_count, "file", "files"),
            byte_count_iec(self.descendant_files_total_size)
        )?;
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

#[derive(Serialize)]
struct FileInfoOutput {
    path: String,
    r#type: String,
    size: i64,
    id: String,
    content_sha1: String,
    content_sha256: String,
}

impl Render for FileInfoOutput {
    fn render(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        write!(w, "Path: {}\n", self.path)?;
        write!(w, "Type: {}\n", self.r#type)?;
        write!(w, "Id: {}\n", self.id)?;
        write!(w, "Content-SHA1: {}\n", self.content_sha1)?;
        write!(w, "Content-SHA256: {}\n", self.content_sha256)?;
        write!(w, "Size: {}\n", byte_count_iec(self.size))?;
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

async fn commit_info(
    matches: &ArgMatches<'_>,
    connection: Connection,
    repo: thrift::RepoSpecifier,
) -> Result<RenderStream, Error> {
    let commit_id = get_commit_id(matches)?;
    let id = resolve_commit_id(&connection, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let params = thrift::CommitInfoParams {
        identity_schemes: get_request_schemes(&matches),
        ..Default::default()
    };
    let response = connection.commit_info(&commit, &params).await?;

    let commit_info = CommitInfo::try_from(&response)?;
    let output = Box::new(CommitInfoOutput {
        commit: commit_info,
        requested: commit_id.to_string(),
        schemes: get_schemes(matches),
    });
    Ok(stream::once(async move { Ok(output as Box<dyn Render>) }).boxed())
}

async fn path_info(
    matches: &ArgMatches<'_>,
    connection: Connection,
    repo: thrift::RepoSpecifier,
    path: String,
) -> Result<RenderStream, Error> {
    let commit_id = get_commit_id(matches)?;
    let id = resolve_commit_id(&connection, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let commit_path = thrift::CommitPathSpecifier {
        commit,
        path: path.clone(),
        ..Default::default()
    };
    let params = thrift::CommitPathInfoParams {
        ..Default::default()
    };
    let response = connection.commit_path_info(&commit_path, &params).await?;
    if response.exists {
        match (response.r#type, response.info) {
            (Some(entry_type), Some(thrift::EntryInfo::tree(info))) => {
                Ok(stream::once(async move { tree_info(path, entry_type, info) }).boxed())
            }
            (Some(entry_type), Some(thrift::EntryInfo::file(info))) => {
                Ok(stream::once(async move { file_info(path, entry_type, info) }).boxed())
            }
            _ => {
                bail!("malformed response for '{}' in {}", path, commit_id);
            }
        }
    } else {
        bail!("'{}' does not exist in {}", path, commit_id);
    }
}

async fn multiple_path_info(
    matches: &ArgMatches<'_>,
    connection: Connection,
    repo: thrift::RepoSpecifier,
    paths: Vec<String>,
) -> Result<RenderStream, Error> {
    let commit_id = get_commit_id(matches)?;
    let id = resolve_commit_id(&connection, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let params = thrift::CommitMultiplePathInfoParams {
        paths,
        ..Default::default()
    };
    let response = connection
        .commit_multiple_path_info(&commit, &params)
        .await?;

    let output = stream::iter(response.path_info)
        .map(move |entry| {
            let (path, commit_info) = entry;
            match (commit_info.r#type, commit_info.info) {
                (Some(entry_type), Some(thrift::EntryInfo::tree(info))) => {
                    tree_info(path, entry_type, info)
                }
                (Some(entry_type), Some(thrift::EntryInfo::file(info))) => {
                    file_info(path, entry_type, info)
                }
                _ => {
                    bail!("malformed response for '{}'", path);
                }
            }
        })
        .boxed();

    Ok(output)
}

fn tree_info(
    path: String,
    entry_type: thrift::EntryType,
    info: thrift::TreeInfo,
) -> Result<Box<dyn Render>, Error> {
    let id = faster_hex::hex_string(&info.id);
    let simple_format_sha1 = faster_hex::hex_string(&info.simple_format_sha1);
    let simple_format_sha256 = faster_hex::hex_string(&info.simple_format_sha256);
    let output = Box::new(TreeInfoOutput {
        path,
        r#type: entry_type.to_string().to_lowercase(),
        id,
        simple_format_sha1,
        simple_format_sha256,
        child_files_count: info.child_files_count,
        child_dirs_count: info.child_dirs_count,
        child_files_total_size: info.child_files_total_size,
        descendant_files_count: info.descendant_files_count,
        descendant_files_total_size: info.descendant_files_total_size,
    });
    Ok(output as Box<dyn Render>)
}

fn file_info(
    path: String,
    entry_type: thrift::EntryType,
    info: thrift::FileInfo,
) -> Result<Box<dyn Render>, Error> {
    let id = faster_hex::hex_string(&info.id);
    let content_sha1 = faster_hex::hex_string(&info.content_sha1);
    let content_sha256 = faster_hex::hex_string(&info.content_sha256);
    let output = Box::new(FileInfoOutput {
        path,
        r#type: entry_type.to_string().to_lowercase(),
        id,
        content_sha1,
        content_sha256,
        size: info.file_size,
    });
    Ok(output as Box<dyn Render>)
}

pub(super) async fn run(
    matches: &ArgMatches<'_>,
    connection: Connection,
) -> Result<RenderStream, Error> {
    let repo = get_repo_specifier(matches).expect("repository is required");

    match get_paths(matches) {
        Some(paths) => {
            let path_vecs = paths.to_vec();
            if path_vecs.len() == 1 {
                let path = &path_vecs[0];
                path_info(matches, connection, repo, path.to_string()).await
            } else {
                multiple_path_info(matches, connection, repo, path_vecs).await
            }
        }
        None => commit_info(matches, connection, repo).await,
    }
}
