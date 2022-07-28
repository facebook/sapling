/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Recursively fetch the contents of a directory.

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use bytesize::ByteSize;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::pin_mut;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use source_control::types as thrift;
use std::borrow::Cow;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;

use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdArgs;
use crate::args::path::PathArgs;
use crate::args::progress::ProgressArgs;
use crate::args::progress::ProgressOutput;
use crate::args::repo::RepoArgs;
use crate::connection::Connection;
use crate::lib::path_tree::PathItem;
use crate::lib::path_tree::PathTree;
use crate::render::Render;
use crate::ScscApp;

/// Chunk size for requests.
const TREE_CHUNK_SIZE: i64 = source_control::TREE_LIST_MAX_LIMIT;
const FILE_CHUNK_SIZE: i64 = source_control::FILE_CONTENT_CHUNK_RECOMMENDED_SIZE;

/// Number of concurrent fetches.
const CONCURRENT_TREE_FETCHES: usize = 4;
const CONCURRENT_FILE_FETCHES: usize = 4;

#[derive(clap::Parser)]
/// Recursively fetch the contents of a directory
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(flatten)]
    path_args: PathArgs,
    #[clap(flatten)]
    progress_args: ProgressArgs,
    #[clap(long, short)]
    /// Destination to export to
    output: PathBuf,
    #[clap(long, short)]
    /// Show paths of files fetched
    verbose: bool,
    #[clap(long)]
    /// Create parent directories of the destination if they do not exist
    make_parent_dirs: bool,
    #[clap(long)]
    /// Filename of a file containing a list of paths (relative to PATH) to export
    path_list_file: Option<String>,
    #[clap(long)]
    /// Perform additional requests to try for case insensitive matches
    case_insensitive: bool,
}

/// Returns a stream of the names of the entries in a single directory `path`.
async fn stream_tree_elements(
    connection: &Connection,
    commit: &thrift::CommitSpecifier,
    path: &str,
) -> Result<impl Stream<Item = Result<String>>> {
    let tree = thrift::TreeSpecifier::by_commit_path(thrift::CommitPathSpecifier {
        commit: commit.clone(),
        path: path.to_string(),
        ..Default::default()
    });
    let response = connection
        .tree_list(
            &tree,
            &thrift::TreeListParams {
                offset: 0,
                limit: source_control::TREE_LIST_MAX_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    Ok(stream::iter(response.entries)
        .map(|entry| Ok(entry.name))
        .chain(
            stream::iter(
                (source_control::TREE_LIST_MAX_LIMIT..response.count)
                    .step_by(source_control::TREE_LIST_MAX_LIMIT as usize),
            )
            .map({
                let connection = connection.clone();
                move |offset| {
                    connection.tree_list(
                        &tree,
                        &thrift::TreeListParams {
                            offset,
                            limit: source_control::TREE_LIST_MAX_LIMIT,
                            ..Default::default()
                        },
                    )
                }
            })
            .buffered(10)
            .and_then(move |response| async move {
                Ok(stream::iter(response.entries).map(|entry| Ok(entry.name)))
            })
            .try_flatten(),
        ))
}

/// Mode of casefold operation for path element comparisons.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Casefold {
    /// Paths should be compared case sensitively.
    Sensitive,

    /// Paths should be compared by folding to lowercase and comparing
    /// the lowercase version.
    Insensitive,
}

impl Casefold {
    /// Returns the appropriate case-folded version of a given path element.
    fn of<'a>(self, s: impl Into<Cow<'a, str>>) -> Cow<'a, str> {
        match self {
            Casefold::Sensitive => s.into(),
            Casefold::Insensitive => s.into().to_lowercase().into(),
        }
    }
}

/// Returns an arbitrary case insensitive match of `subpath` within the (case
/// sensitive) `target_dir`, or `None` if there is no such match.
fn case_insensitive_subpath<'a>(
    connection: &'a Connection,
    commit: &'a thrift::CommitSpecifier,
    target_dir: &'a str,
    subpath: &'a str,
) -> BoxFuture<'a, Result<Option<String>>> {
    async move {
        let (target_elem, target_subpath) = subpath.split_once('/').unwrap_or((subpath, ""));
        let target_elem_lower = target_elem.to_lowercase();
        let elements = stream_tree_elements(connection, commit, target_dir).await?;
        pin_mut!(elements);
        while let Some(elem) = elements.try_next().await? {
            if elem.to_lowercase() == target_elem_lower {
                let target = format!("{}/{}", target_dir, elem);
                if target_subpath.is_empty() {
                    return Ok(Some(target));
                } else if let Some(response) =
                    // We've found a possible directory to look into - recurse
                    // to see if we can find the full path.
                    case_insensitive_subpath(
                        connection,
                        commit,
                        &target,
                        target_subpath,
                    )
                    .await?
                {
                    return Ok(Some(response));
                }
            }
        }
        Ok(None)
    }
    .boxed()
}

/// Returns an iterator over pairs of each of the path prefixes of `path` and
/// the subpath within that prefix, finishing with the full path relative to
/// the root.  Initial trailing slashes are removed from the path.
///
/// ```
/// assert_eq!(
///    iter_path_prefixes("a/b/c").collect::<Vec<_>>,
///    vec![("a/b", "c"), ("a", "b/c"), ("", "a/b/c")],
/// );
/// ```
fn iter_path_prefixes(path: &str) -> impl Iterator<Item = (&str, &str)> {
    let path = path.trim_end_matches('/');
    path.rmatch_indices('/')
        .map(|(slash, _)| (&path[..slash], &path[slash + 1..]))
        .chain(Some(("", path)))
}

/// Returns an arbitrary case-insensitive match of `path`, or `None` if there
/// is no such match.
async fn case_insensitive_path(
    connection: &Connection,
    commit: &thrift::CommitSpecifier,
    path: &str,
) -> Result<Option<String>> {
    // Heuristic: typically it's the last few path elements that actually need
    // casefolding, so start from the end of the path and look up parent
    // directories one by one.
    for (target_dir, target_subpath) in iter_path_prefixes(path) {
        let (target_elem, target_subpath) = target_subpath
            .split_once('/')
            .unwrap_or((target_subpath, ""));
        let target_elem_lower = target_elem.to_lowercase();
        let elements = stream_tree_elements(connection, commit, target_dir).await?;
        pin_mut!(elements);
        while let Some(elem) = elements.try_next().await? {
            if elem.to_lowercase() == target_elem_lower {
                let target = format!("{}/{}", target_dir, elem);
                if let Some(response) =
                    case_insensitive_subpath(connection, commit, &target, target_subpath).await?
                {
                    return Ok(Some(response));
                }
            }
        }
    }
    Ok(None)
}

fn join_path(path: &str, elem: &str) -> String {
    let mut path = path.to_string();
    if !path.is_empty() && !path.ends_with('/') {
        path.push('/');
    }
    path.push_str(elem);
    path
}

fn export_tree_entry(
    path: &str,
    destination: &Path,
    entry: thrift::TreeEntry,
) -> Result<ExportItem> {
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
    casefold: Casefold,
) -> Result<Option<ExportItem>> {
    match (filter.remove(&casefold.of(&entry.name)), entry.info) {
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
    casefold: Casefold,
) -> Result<Vec<ExportItem>> {
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
                        anyhow::Ok(connection.tree_list(&tree, &params).await?.entries)
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
                export_filtered_tree_entry(&path, &destination, entry, &mut filter, casefold)
                    .transpose()
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
) -> Result<()> {
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
                anyhow::Ok(())
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
    casefold: Casefold,
    files_written: Arc<AtomicU64>,
    bytes_written: Arc<AtomicU64>,
) -> Result<(Option<String>, Vec<ExportItem>)> {
    match item {
        ExportItem::Tree {
            path,
            id,
            destination,
            filter,
        } => {
            let items =
                export_tree(connection, repo, path, id, destination, filter, casefold).await?;
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
    type Args = ();

    fn render_tty(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        writeln!(w, "{}", self.path)?;
        Ok(())
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let destination: PathBuf = args.output;
    if destination.exists() {
        bail!(
            "destination ({}) already exists",
            destination.to_string_lossy()
        );
    }
    let casefold = if args.case_insensitive {
        Casefold::Insensitive
    } else {
        Casefold::Sensitive
    };

    let path_tree = match args.path_list_file {
        Some(path_list_file) => {
            let file = tokio::fs::File::open(path_list_file)
                .await
                .context("failed to open path list file")?;
            let lines = tokio::io::BufReader::new(file).lines();
            let stream = tokio_stream::wrappers::LinesStream::new(lines);

            let path_tree = stream
                .map_ok(|path| casefold.of(path).into_owned())
                .try_collect::<PathTree>()
                .await
                .context("failed to load path list file")?;
            Some(path_tree)
        }
        None => None,
    };

    if args.make_parent_dirs {
        if let Some(parent) = destination.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .context("failed to create parent directories")?;
            }
        }
    }

    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let id = resolve_commit_id(&app.connection, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    };
    let path = args.path_args.clone().path;
    let mut commit_path = thrift::CommitPathSpecifier {
        commit: commit.clone(),
        path: path.clone(),
        ..Default::default()
    };

    let params = thrift::CommitPathInfoParams {
        ..Default::default()
    };
    let response = {
        let mut response = app
            .connection
            .commit_path_info(&commit_path, &params)
            .await?;
        if !response.exists && casefold == Casefold::Insensitive {
            if let Some(case_path) = case_insensitive_path(&app.connection, &commit, &path).await? {
                commit_path.path = case_path;
                response = app
                    .connection
                    .commit_path_info(&commit_path, &params)
                    .await?;
            }
        }
        response
    };

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
                return Ok(());
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
                app.connection.clone(),
                repo.clone(),
                item,
                casefold,
                files_written.clone(),
                bytes_written.clone(),
            )
            .boxed()
        }
    });

    let stream = if args.verbose {
        stream
            .try_filter_map(|path| async move { Ok(path.map(|path| ExportedFile { path })) })
            .left_stream()
    } else {
        stream
            .try_filter_map(|_path| async move { Ok(None) })
            .right_stream()
    };

    let render = args.progress_args.render_progress(stream, move || {
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
    });
    app.target.render(&(), render).await
}
