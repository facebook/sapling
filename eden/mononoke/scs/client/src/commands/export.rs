/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Recursively fetch the contents of a directory.

use anyhow::{bail, Context, Error};
use bytesize::ByteSize;
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use futures::future::FutureExt;
use futures::stream::{self, StreamExt, TryStreamExt};
use source_control::types as thrift;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use crate::args::commit_id::{add_commit_id_args, get_commit_id, resolve_commit_id};
use crate::args::path::{add_path_args, get_path};
use crate::args::repo::{add_repo_args, get_repo_specifier};
use crate::connection::Connection;
use crate::lib::path_tree::{PathItem, PathTree};
use crate::lib::progress::{add_progress_args, progress_renderer, ProgressOutput};
use crate::render::{Render, RenderStream};

pub(super) const NAME: &str = "export";

const ARG_OUTPUT: &str = "OUTPUT";
const ARG_VERBOSE: &str = "VERBOSE";
const ARG_MAKE_PARENT_DIRS: &str = "MAKE_PARENT_DIRS";
const ARG_PATH_LIST_FILE: &str = "PATH_LIST_FILE";

/// Chunk size for requests.
const TREE_CHUNK_SIZE: i64 = source_control::TREE_LIST_MAX_LIMIT;
const FILE_CHUNK_SIZE: i64 = source_control::FILE_CONTENT_CHUNK_RECOMMENDED_SIZE;

/// Number of concurrent fetches.
const CONCURRENT_TREE_FETCHES: usize = 4;
const CONCURRENT_FILE_FETCHES: usize = 4;

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Recursively fetch the contents of a directory")
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_commit_id_args(cmd);
    let cmd = add_path_args(cmd);
    let cmd = add_progress_args(cmd);
    let cmd = cmd.arg(
        Arg::with_name(ARG_OUTPUT)
            .short("o")
            .long("output")
            .takes_value(true)
            .number_of_values(1)
            .required(true)
            .help("Destination to export to"),
    );
    let cmd = cmd.arg(
        Arg::with_name(ARG_VERBOSE)
            .short("v")
            .long("verbose")
            .help("Show paths of files fetched"),
    );
    let cmd = cmd.arg(
        Arg::with_name(ARG_MAKE_PARENT_DIRS)
            .long("make-parent-dirs")
            .help("Create parent directories of the destination if they do not exist"),
    );
    let cmd = cmd.arg(
        Arg::with_name(ARG_PATH_LIST_FILE)
            .long("path-list-file")
            .takes_value(true)
            .help("Filename of a file containing a list of paths (relative to PATH) to export"),
    );
    cmd
}

fn join_path(path: &str, elem: &str) -> String {
    let mut path = path.to_string();
    if !path.is_empty() && !path.ends_with("/") {
        path.push_str("/");
    }
    path.push_str(elem);
    path
}

fn export_tree_entry(
    path: &str,
    destination: &Path,
    entry: thrift::TreeEntry,
) -> Result<ExportItem, Error> {
    match entry.info {
        thrift::EntryInfo::tree(info) => Ok(ExportItem::Tree {
            path: join_path(path, &entry.name),
            id: info.id,
            destination: destination.join(&entry.name),
            filter: None,
        }),
        thrift::EntryInfo::file(info) => Ok(ExportItem::File {
            path: join_path(path, &entry.name),
            id: info.id,
            destination: destination.join(&entry.name),
            size: info.file_size as u64,
            type_: entry.r#type,
        }),
        _ => {
            bail!("malformed response format for '{}'", entry.name);
        }
    }
}

fn export_filtered_tree_entry(
    path: &str,
    destination: &Path,
    entry: thrift::TreeEntry,
    filter: &mut PathTree,
) -> Result<Option<ExportItem>, Error> {
    match (filter.remove(&entry.name), entry.info) {
        (None, _) => Ok(None),
        (Some(item), thrift::EntryInfo::tree(info)) => {
            let subfilter = match item {
                PathItem::Target | PathItem::TargetDir => None,
                PathItem::Dir(tree) => Some(tree),
            };
            Ok(Some(ExportItem::Tree {
                path: join_path(path, &entry.name),
                id: info.id,
                destination: destination.join(&entry.name),
                filter: subfilter,
            }))
        }
        (Some(PathItem::Dir(_) | PathItem::TargetDir), thrift::EntryInfo::file(_)) => Ok(None),
        (Some(PathItem::Target), thrift::EntryInfo::file(info)) => Ok(Some(ExportItem::File {
            path: join_path(path, &entry.name),
            id: info.id,
            destination: destination.join(&entry.name),
            size: info.file_size as u64,
            type_: entry.r#type,
        })),
        _ => bail!("malformed response format for '{}'", entry.name),
    }
}

async fn export_tree(
    connection: Connection,
    repo: thrift::RepoSpecifier,
    path: String,
    id: Vec<u8>,
    destination: PathBuf,
    filter: Option<PathTree>,
) -> Result<Vec<ExportItem>, Error> {
    tokio::fs::create_dir(&destination).await?;
    let tree = thrift::TreeSpecifier::by_id(thrift::TreeIdSpecifier {
        repo,
        id,
        ..Default::default()
    });
    let params = thrift::TreeListParams {
        offset: 0,
        limit: TREE_CHUNK_SIZE,
        ..Default::default()
    };
    let response = connection.tree_list(&tree, &params).await?;
    let count = response.count;
    let other_tree_chunks =
        stream::iter((TREE_CHUNK_SIZE..count).step_by(TREE_CHUNK_SIZE as usize))
            .map({
                |offset| {
                    // Request subsequent chunks of the directory listing.
                    let connection = connection.clone();
                    let tree = tree.clone();
                    async move {
                        let params = thrift::TreeListParams {
                            offset,
                            limit: TREE_CHUNK_SIZE,
                            ..Default::default()
                        };
                        Ok::<_, Error>(connection.tree_list(&tree, &params).await?.entries)
                    }
                }
            })
            .buffered(CONCURRENT_TREE_FETCHES)
            .try_collect::<Vec<_>>()
            .await?;

    let output = if let Some(mut filter) = filter {
        Some(response.entries)
            .into_iter()
            .chain(other_tree_chunks)
            .flatten()
            .filter_map(|entry| {
                export_filtered_tree_entry(&path, &destination, entry, &mut filter).transpose()
            })
            .collect::<Result<_, _>>()?
    } else {
        Some(response.entries)
            .into_iter()
            .chain(other_tree_chunks)
            .flatten()
            .map(|entry| export_tree_entry(&path, &destination, entry))
            .collect::<Result<_, _>>()?
    };
    Ok(output)
}

async fn export_file(
    connection: Connection,
    repo: thrift::RepoSpecifier,
    id: Vec<u8>,
    destination: PathBuf,
    size: u64,
    type_: thrift::EntryType,
    bytes_written: &Arc<AtomicU64>,
) -> Result<(), Error> {
    let file = thrift::FileSpecifier::by_id(thrift::FileIdSpecifier {
        repo,
        id,
        ..Default::default()
    });
    let mut responses = stream::iter((0..size).step_by(FILE_CHUNK_SIZE as usize))
        .map({
            move |offset| {
                let params = thrift::FileContentChunkParams {
                    offset: offset as i64,
                    size: FILE_CHUNK_SIZE,
                    ..Default::default()
                };
                connection.file_content_chunk(&file, &params)
            }
        })
        .buffered(CONCURRENT_FILE_FETCHES);

    #[cfg(unix)]
    {
        if type_ == thrift::EntryType::LINK {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            let mut target = Vec::new();
            while let Some(response) = responses.try_next().await? {
                target.extend_from_slice(response.data.as_slice());
            }
            tokio::fs::symlink(OsStr::from_bytes(target.as_slice()), &destination).await?;
            bytes_written.fetch_add(size, Ordering::Relaxed);
            return Ok(());
        }
    }

    let mut out_file = tokio::fs::File::create(&destination).await?;
    while let Some(response) = responses.try_next().await? {
        let len = response.data.len() as u64;
        out_file.write_all(&response.data).await?;
        bytes_written.fetch_add(len, Ordering::Relaxed);
    }

    #[cfg(unix)]
    {
        if type_ == thrift::EntryType::EXEC {
            // Tokio doesn't support setting permissions yet, so we must use
            // the standard library.
            use std::os::unix::fs::PermissionsExt;
            let out_file = out_file.into_std().await;
            tokio::task::spawn_blocking(move || {
                let metadata = out_file.metadata()?;
                let mut permissions = metadata.permissions();
                let mode = permissions.mode();
                // Propagate read permissions to execute permissions.
                permissions.set_mode(mode | ((mode & 0o444) >> 2));
                std::fs::set_permissions(&destination, permissions)?;
                Ok::<_, Error>(())
            })
            .await??;
        }
    }

    Ok(())
}

async fn export_item(
    connection: Connection,
    repo: thrift::RepoSpecifier,
    item: ExportItem,
    files_written: Arc<AtomicU64>,
    bytes_written: Arc<AtomicU64>,
) -> Result<(Option<String>, Vec<ExportItem>), Error> {
    match item {
        ExportItem::Tree {
            path,
            id,
            destination,
            filter,
        } => {
            let items = export_tree(connection, repo, path, id, destination, filter).await?;
            Ok((None, items))
        }
        ExportItem::File {
            path,
            id,
            destination,
            size,
            type_,
        } => {
            export_file(
                connection,
                repo,
                id,
                destination,
                size,
                type_,
                &bytes_written,
            )
            .await?;
            files_written.fetch_add(1, Ordering::Relaxed);
            Ok((Some(path), Vec::new()))
        }
    }
}

enum ExportItem {
    Tree {
        path: String,
        id: Vec<u8>,
        destination: PathBuf,
        filter: Option<PathTree>,
    },
    File {
        path: String,
        id: Vec<u8>,
        destination: PathBuf,
        size: u64,
        type_: thrift::EntryType,
    },
}

struct ExportedFile {
    path: String,
}

impl Render for ExportedFile {
    fn render_tty(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        writeln!(w, "{}", self.path)?;
        Ok(())
    }
}

pub(super) async fn run(
    matches: &ArgMatches<'_>,
    connection: Connection,
) -> Result<RenderStream, Error> {
    let destination: PathBuf = matches
        .value_of_os(ARG_OUTPUT)
        .expect("destination is required")
        .into();
    if destination.exists() {
        bail!(
            "destination ({}) already exists",
            destination.to_string_lossy()
        );
    }
    let path_tree = match matches.value_of_os(ARG_PATH_LIST_FILE) {
        Some(path_list_file) => {
            let file = tokio::fs::File::open(path_list_file)
                .await
                .context("failed to open path list file")?;
            let lines = tokio::io::BufReader::new(file).lines();
            let stream = tokio_stream::wrappers::LinesStream::new(lines);
            let path_tree = stream
                .try_collect::<PathTree>()
                .await
                .context("failed to load path list file")?;
            Some(path_tree)
        }
        None => None,
    };

    if matches.is_present(ARG_MAKE_PARENT_DIRS) {
        if let Some(parent) = destination.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .context("failed to create parent directories")?;
            }
        }
    }

    let repo = get_repo_specifier(matches).expect("repository is required");
    let commit_id = get_commit_id(matches)?;
    let id = resolve_commit_id(&connection, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    };
    let path = get_path(matches).expect("path is required");
    let commit_path = thrift::CommitPathSpecifier {
        commit,
        path: path.clone(),
        ..Default::default()
    };

    let params = thrift::CommitPathInfoParams {
        ..Default::default()
    };
    let response = connection.commit_path_info(&commit_path, &params).await?;

    if !response.exists {
        bail!("'{}' does not exist in {}", path, commit_id);
    }

    let file_count;
    let total_size;
    let files_written = Arc::new(AtomicU64::new(0));
    let bytes_written = Arc::new(AtomicU64::new(0));

    let item = match (response.r#type, response.info) {
        (Some(_type), Some(thrift::EntryInfo::tree(info))) => {
            file_count = info.descendant_files_count as u64;
            total_size = info.descendant_files_total_size as u64;
            ExportItem::Tree {
                path,
                id: info.id,
                destination,
                filter: path_tree,
            }
        }
        (Some(type_), Some(thrift::EntryInfo::file(info))) => {
            file_count = 1;
            total_size = info.file_size as u64;
            if path_tree.is_some() {
                // A list of paths to filter has been provided, but the target
                // is a file, so none of the paths can possible match.
                return Ok(stream::empty().boxed());
            }
            ExportItem::File {
                path,
                id: info.id,
                destination,
                size: info.file_size as u64,
                type_,
            }
        }
        _ => {
            bail!("malformed response for '{}' in {}", path, commit_id);
        }
    };

    let stream = bounded_traversal::bounded_traversal_stream(100, Some(item), {
        let files_written = files_written.clone();
        let bytes_written = bytes_written.clone();
        move |item| {
            export_item(
                connection.clone(),
                repo.clone(),
                item,
                files_written.clone(),
                bytes_written.clone(),
            )
            .boxed()
        }
    });

    let stream = if matches.is_present(ARG_VERBOSE) {
        stream
            .try_filter_map(|path| async move {
                Ok(path.map(|path| Box::new(ExportedFile { path }) as Box<dyn Render>))
            })
            .left_stream()
    } else {
        stream
            .try_filter_map(|_path| async move { Ok(None) })
            .right_stream()
    };

    Ok(progress_renderer(matches, stream, move || {
        let files_written = files_written.load(Ordering::Relaxed);
        let bytes_written = bytes_written.load(Ordering::Relaxed);
        let message = format!(
            "Exported {:>5}/{:>5} files, {:>8}/{:>8}",
            files_written,
            file_count,
            ByteSize::b(bytes_written).to_string_as(true),
            ByteSize::b(total_size).to_string_as(true),
        );
        ProgressOutput::new(message, bytes_written, total_size)
    }))
}
