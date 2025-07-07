/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Recursively fetch the contents of a directory.

use std::borrow::Cow;
use std::future;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use bytesize::ByteSize;
use cloned::cloned;
use commit_id_types::CommitIdArgs;
use futures::AsyncWrite;
use futures::TryFutureExt;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::pin_mut;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use scs_client_raw::ScsClient;
use scs_client_raw::thrift;
use source_control::FileChunk;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufWriter;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use crate::ScscApp;
use crate::args::commit_id::resolve_commit_id;
use crate::args::path::PathArgs;
use crate::args::progress::ProgressArgs;
use crate::args::progress::ProgressOutput;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::library::path_tree::PathItem;
use crate::library::path_tree::PathTree;
use crate::render::Render;

/// Chunk size for requests.
const TREE_CHUNK_SIZE: i64 = source_control::TREE_LIST_MAX_LIMIT;
const FILE_CHUNK_SIZE: i64 = source_control::FILE_CONTENT_CHUNK_RECOMMENDED_SIZE;

/// Number of concurrent fetches.
const CONCURRENT_TREE_FETCHES: usize = 10;
/// How many chunks for single file to buffer ahead.
const WRITER_CHUNK_BUFFER_SIZE: usize = 5;
/// How many file handles to buffer when traversing the tree.
const READY_TO_DOWNLOAD_FILE_STREAM_BUFFER_SIZE: usize = 100;
/// How many download chunks to buffer ahead.
const DOWNLOADER_OUTPUT_CHUNK_BUFFER_SIZE: usize = 25;
/// How many files can be written concurrently to disk.
const CONCURRENT_FILE_WRITES: usize = 30;

#[derive(Clone)]
struct FileMetadata {
    size: u64,
    path: PathBuf,
    entry_type: thrift::EntryType,
}

type FileSender =
    mpsc::Sender<BoxStream<'static, BoxFuture<'static, anyhow::Result<(FileMetadata, FileChunk)>>>>;

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
    /// Destination to export to ("-" for stdout, otherwise path)
    output: String,
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
    #[clap(long)]
    /// Rather than downloading to a directory, create a tar archive
    tar: bool,
    /// Concurrent file fetches (multiply by 50MB to get extra memory footprint)
    #[clap(long, default_value_t = 40)]
    concurrent_file_fetches: usize,
}

/// Returns a stream of the names of the entries in a single directory `path`.
async fn stream_tree_elements(
    connection: &ScsClient,
    commit: &thrift::CommitSpecifier,
    path: &str,
) -> Result<impl Stream<Item = Result<String>> + use<>> {
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
        .await
        .map_err(|e| e.handle_selection_error(&commit.repo))?;

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

/// Whether to create dirs at destination
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum CreateDirs {
    Yes,
    No,
}

/// Returns an arbitrary case insensitive match of `subpath` within the (case
/// sensitive) `target_dir`, or `None` if there is no such match.
fn case_insensitive_subpath<'a>(
    connection: &'a ScsClient,
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
    connection: &ScsClient,
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
    tx: FileSender,
    destination: &Path,
    entry: thrift::TreeEntry,
) -> Result<ExportItem> {
    match entry.info {
        thrift::EntryInfo::tree(info) => Ok(ExportItem::Tree {
            path: join_path(path, &entry.name),
            id: info.id,
            tx,
            destination: destination.join(&entry.name),
            filter: None,
        }),
        thrift::EntryInfo::file(info) => Ok(ExportItem::File {
            path: join_path(path, &entry.name),
            id: info.id,
            tx,
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
    tx: FileSender,
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
                tx,
                destination: destination.join(&entry.name),
                filter: subfilter,
            }))
        }
        (Some(PathItem::Dir(_) | PathItem::TargetDir), thrift::EntryInfo::file(_)) => Ok(None),
        (Some(PathItem::Target), thrift::EntryInfo::file(info)) => Ok(Some(ExportItem::File {
            path: join_path(path, &entry.name),
            id: info.id,
            tx,
            destination: destination.join(&entry.name),
            size: info.file_size as u64,
            type_: entry.r#type,
        })),
        _ => bail!("malformed response format for '{}'", entry.name),
    }
}

async fn export_tree(
    connection: ScsClient,
    repo: thrift::RepoSpecifier,
    path: String,
    id: Vec<u8>,
    tx: FileSender,
    destination: PathBuf,
    filter: Option<PathTree>,
    casefold: Casefold,
    create_dirs: CreateDirs,
) -> Result<Vec<ExportItem>> {
    if create_dirs == CreateDirs::Yes {
        tokio::fs::create_dir(&destination).await?;
    }
    let tree = thrift::TreeSpecifier::by_id(thrift::TreeIdSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    });
    let params = thrift::TreeListParams {
        offset: 0,
        limit: TREE_CHUNK_SIZE,
        ..Default::default()
    };
    let response = connection
        .tree_list(&tree, &params)
        .await
        .map_err(|e| e.handle_selection_error(&repo))?;
    let count = response.count;
    let other_tree_chunks =
        stream::iter((TREE_CHUNK_SIZE..count).step_by(TREE_CHUNK_SIZE as usize))
            .map({
                |offset| {
                    cloned!(repo);
                    // Request subsequent chunks of the directory listing.
                    let connection = connection.clone();
                    let tree = tree.clone();
                    async move {
                        let params = thrift::TreeListParams {
                            offset,
                            limit: TREE_CHUNK_SIZE,
                            ..Default::default()
                        };
                        anyhow::Ok(
                            connection
                                .tree_list(&tree, &params)
                                .await
                                .map_err(|e| e.handle_selection_error(&repo))?
                                .entries,
                        )
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
                export_filtered_tree_entry(
                    &path,
                    tx.clone(),
                    &destination,
                    entry,
                    &mut filter,
                    casefold,
                )
                .transpose()
            })
            .collect::<Result<_, _>>()?
    } else {
        Some(response.entries)
            .into_iter()
            .chain(other_tree_chunks)
            .flatten()
            .map(|entry| export_tree_entry(&path, tx.clone(), &destination, entry))
            .collect::<Result<_, _>>()?
    };
    Ok(output)
}

async fn export_file(
    connection: ScsClient,
    repo: thrift::RepoSpecifier,
    id: Vec<u8>,
    tx: FileSender,
    destination: PathBuf,
    size: u64,
    _type_: thrift::EntryType,
) -> Result<()> {
    let file = thrift::FileSpecifier::by_id(thrift::FileIdSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    });
    let file_metadata = FileMetadata {
        size,
        path: destination.clone(),
        entry_type: _type_,
    };
    let responses = if size > 0 {
        stream::iter((0..size).step_by(FILE_CHUNK_SIZE as usize))
            .map({
                move |offset| {
                    cloned!(repo);
                    let params = thrift::FileContentChunkParams {
                        offset: offset as i64,
                        size: FILE_CHUNK_SIZE,
                        ..Default::default()
                    };
                    connection
                        .file_content_chunk(&file, &params)
                        .map_err(move |e| e.handle_selection_error(&repo))
                        .map_ok({
                            cloned!(file_metadata);
                            move |chunk| (file_metadata, chunk)
                        })
                        .boxed()
                }
            })
            .left_stream()
    } else {
        // Even though they have no content we have to emit empty files to the
        // metadata gets through
        stream::once(future::ready(
            future::ready(anyhow::Ok((
                file_metadata,
                FileChunk {
                    offset: 0,
                    file_size: 0,
                    data: vec![],
                    ..Default::default()
                },
            )))
            .boxed(),
        ))
        .right_stream()
    };

    let _ = tx.send(Box::pin(responses)).await;

    Ok(())
}

async fn export_item(
    connection: ScsClient,
    repo: thrift::RepoSpecifier,
    item: ExportItem,
    casefold: Casefold,
    create_dirs: CreateDirs,
    files_written: Arc<AtomicU64>,
) -> Result<(Option<String>, Vec<ExportItem>)> {
    match item {
        ExportItem::Tree {
            path,
            id,
            tx,
            destination,
            filter,
        } => {
            let items = export_tree(
                connection,
                repo,
                path,
                id,
                tx,
                destination,
                filter,
                casefold,
                create_dirs,
            )
            .await?;
            Ok((None, items))
        }
        ExportItem::File {
            path,
            id,
            tx,
            destination,
            size,
            type_,
        } => {
            export_file(connection, repo, id, tx, destination, size, type_).await?;
            files_written.fetch_add(1, Ordering::Relaxed);
            Ok((Some(path), Vec::new()))
        }
    }
}

enum ExportItem {
    Tree {
        path: String,
        id: Vec<u8>,
        tx: FileSender,
        destination: PathBuf,
        filter: Option<PathTree>,
    },
    File {
        path: String,
        id: Vec<u8>,
        tx: FileSender,
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

async fn downloader(
    rx: mpsc::Receiver<
        BoxStream<'static, BoxFuture<'static, anyhow::Result<(FileMetadata, FileChunk)>>>,
    >,
    tx: mpsc::Sender<(FileMetadata, FileChunk)>,
    concurrent_file_fetches: usize,
) -> anyhow::Result<()> {
    let mut flattened_stream = ReceiverStream::new(rx)
        .flatten()
        .buffered(concurrent_file_fetches);

    while let Some(item) = flattened_stream.try_next().await? {
        tx.send(item).await?;
    }
    Ok(())
}

async fn archive_writer<W: AsyncWrite + Unpin + Send + Sync + 'static>(
    mut receiver: mpsc::Receiver<(FileMetadata, FileChunk)>,
    archive: async_tar::Builder<W>,
    bytes_written: Arc<AtomicU64>,
) -> anyhow::Result<()> {
    // Setup initial state
    let mut last_path: Option<PathBuf> = None;
    // throwaway channel (so we don't need to use optional and overcomplicate code later)
    #[allow(unused_assignments)]
    let (mut tx, mut rx) = mpsc::channel(WRITER_CHUNK_BUFFER_SIZE);
    let mut fut = Box::new(tokio::spawn(async move { std::io::Result::Ok(archive) }));

    while let Some((file_metadata, chunk)) = receiver.recv().await {
        if last_path.as_ref() != Some(&file_metadata.path) {
            // Await previous write that should return the archive handle back
            drop(tx);
            let mut archive = fut.await??;

            // Create new channels for next path to write
            (tx, rx) = mpsc::channel::<Result<Vec<u8>, std::io::Error>>(WRITER_CHUNK_BUFFER_SIZE);

            // Kick off the next write
            {
                cloned!(file_metadata);
                let mut header = async_tar::Header::new_gnu();
                header.set_size(file_metadata.size);
                header.set_cksum();

                match file_metadata.entry_type {
                    thrift::EntryType::EXEC => {
                        header.set_mode(0o755);
                    }
                    _ => {
                        header.set_mode(0o644);
                    }
                }

                fut = Box::new(tokio::spawn(async move {
                    archive
                        .append_data(
                            &mut header,
                            file_metadata.path.clone(),
                            ReceiverStream::new(rx).into_async_read(),
                        )
                        .await?;
                    Ok(archive)
                }));
            }
        }
        let len = chunk.data.len() as u64;
        tx.send(Ok(chunk.data)).await?;
        bytes_written.fetch_add(len, Ordering::Relaxed);
        last_path = Some(file_metadata.path);
    }
    drop(tx);
    // Finish last write. We don't need archive anymore.
    let _archive = fut.await?;
    Ok(())
}

async fn filesystem_writer(
    mut receiver: mpsc::Receiver<(FileMetadata, FileChunk)>,
    bytes_written: Arc<AtomicU64>,
) -> anyhow::Result<()> {
    // Setup initial state
    let mut last_path: Option<PathBuf> = None;

    // throwaway channel (so we don't need to use optional and overcomplicate code later)
    #[allow(unused_assignments)]
    let (mut chunks_tx, mut chunks_rx) = mpsc::channel(WRITER_CHUNK_BUFFER_SIZE);

    // channel with all pending file writes, once it's empty we finished all the writes
    let (file_writes_tx, file_writes_rx) = mpsc::channel(CONCURRENT_FILE_WRITES);
    let file_writes: tokio::task::JoinHandle<std::result::Result<(), anyhow::Error>> = tokio::spawn(
        ReceiverStream::new(file_writes_rx)
            .map(Ok)
            .try_for_each(|fut| async move { fut.await? }),
    );

    while let Some((file_metadata, chunk)) = receiver.recv().await {
        if last_path.as_ref() != Some(&file_metadata.path) {
            drop(chunks_tx);

            // Create new channels for next path to write
            (chunks_tx, chunks_rx) = mpsc::channel::<Vec<u8>>(WRITER_CHUNK_BUFFER_SIZE);

            // Kick off the next write
            filesystem_write_file(&file_writes_tx, file_metadata.clone(), chunks_rx).await?;
        }
        let len = chunk.data.len() as u64;
        chunks_tx.send(chunk.data).await?;
        bytes_written.fetch_add(len, Ordering::Relaxed);
        last_path = Some(file_metadata.path);
    }
    drop(chunks_tx);
    drop(file_writes_tx);
    // Wait for all pending writes to finish
    file_writes.await??;
    Ok(())
}

async fn filesystem_write_file(
    file_writes_tx: &mpsc::Sender<
        Box<tokio::task::JoinHandle<std::result::Result<(), anyhow::Error>>>,
    >,
    file_metadata: FileMetadata,
    mut chunks_rx: mpsc::Receiver<Vec<u8>>,
) -> anyhow::Result<()> {
    #[cfg(unix)]
    if file_metadata.entry_type == thrift::EntryType::LINK {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let fut = Box::new(tokio::spawn(async move {
            let chunks: Vec<Vec<u8>> = ReceiverStream::new(chunks_rx).collect().await;
            let data = chunks.into_iter().flatten().collect::<Vec<u8>>();
            tokio::fs::symlink(OsStr::from_bytes(data.as_slice()), &file_metadata.path).await?;
            Ok(())
        }));
        file_writes_tx.send(fut).await?;
        return Ok(());
    }

    let out_file = tokio::fs::File::create(&file_metadata.path).await?;
    // Create a buffered writer for the file
    let mut writer = BufWriter::new(out_file);
    let fut = Box::new(tokio::spawn(async move {
        while let Some(chunk) = chunks_rx.recv().await {
            writer.write_all(&chunk).await?;
        }
        writer.flush().await?;
        Ok(())
    }));
    file_writes_tx.send(fut).await?;

    #[cfg(unix)]
    if file_metadata.entry_type == thrift::EntryType::EXEC {
        use std::os::unix::fs::PermissionsExt;
        let out_file = tokio::fs::File::open(&file_metadata.path).await?;
        let mut permissions = out_file.metadata().await?.permissions();
        let mode = permissions.mode();
        // Propagate read permissions to execute permissions.
        permissions.set_mode(mode | ((mode & 0o444) >> 2));
        tokio::fs::set_permissions(file_metadata.path, permissions).await?
    }

    Ok(())
}

enum Destination {
    Path(PathBuf),
    Stdout,
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let destination = match args.output.as_ref() {
        "-" => {
            if !args.tar {
                bail!("stdout output requires --tar");
            }
            if args.make_parent_dirs {
                bail!("--make-parent-dirs incompatible with stdout output");
            }
            Destination::Stdout
        }
        path => {
            let path = PathBuf::from(path);
            if path.exists() {
                bail!("destination ({}) already exists", path.to_string_lossy());
            }

            if args.make_parent_dirs {
                if let Some(parent) = path.parent() {
                    if !parent.exists() {
                        tokio::fs::create_dir_all(parent)
                            .await
                            .context("failed to create parent directories")?;
                    }
                }
            }

            Destination::Path(path)
        }
    };

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

    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let conn = app.get_connection(Some(&repo.name))?;
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;
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
        let mut response = conn
            .commit_path_info(&commit_path, &params)
            .await
            .map_err(|e| e.handle_selection_error(&repo))?;
        if !response.exists && casefold == Casefold::Insensitive {
            if let Some(case_path) = case_insensitive_path(&conn, &commit, &path).await? {
                commit_path.path = case_path;
                response = conn
                    .commit_path_info(&commit_path, &params)
                    .await
                    .map_err(|e| e.handle_selection_error(&commit.repo))?;
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

    let (tx, rx) = mpsc::channel(READY_TO_DOWNLOAD_FILE_STREAM_BUFFER_SIZE);
    let (downloader_tx, downloader_rx) = mpsc::channel(DOWNLOADER_OUTPUT_CHUNK_BUFFER_SIZE);
    let downloader = tokio::spawn(downloader(rx, downloader_tx, args.concurrent_file_fetches));

    let (file_writer, create_dirs, root) = if args.tar {
        let handle = match destination {
            Destination::Path(ref path) => tokio::spawn(archive_writer(
                downloader_rx,
                async_tar::Builder::new(tokio::fs::File::create(path).await?.compat_write()),
                bytes_written.clone(),
            )),
            Destination::Stdout => tokio::spawn(archive_writer(
                downloader_rx,
                async_tar::Builder::new(tokio::io::stdout().compat_write()),
                bytes_written.clone(),
            )),
        };

        (
            handle,
            CreateDirs::No,
            // the destination is the internal destination within tar archive
            repo.name.clone().into(),
        )
    } else {
        (
            tokio::spawn(filesystem_writer(downloader_rx, bytes_written.clone())),
            CreateDirs::Yes,
            match destination {
                Destination::Path(path) => path,
                Destination::Stdout => bail!("stdout output requires --tar"),
            },
        )
    };

    let item = match (response.r#type, response.info) {
        (Some(_type), Some(thrift::EntryInfo::tree(info))) => {
            file_count = info.descendant_files_count as u64;
            total_size = info.descendant_files_total_size as u64;
            ExportItem::Tree {
                path,
                id: info.id,
                tx,
                destination: root,
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
                tx,
                destination: root,
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
        move |item| {
            export_item(
                conn.clone(),
                repo.clone(),
                item,
                casefold,
                create_dirs,
                files_written.clone(),
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
            ByteSize::b(bytes_written).display().iec().to_string(),
            ByteSize::b(total_size).display().iec().to_string(),
        );
        ProgressOutput::new(message, bytes_written, total_size)
    });
    app.target.render_stderr(&(), render).await?;
    file_writer.await??;
    downloader.await?
}
