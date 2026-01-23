/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fs::create_dir_all;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::thread;

use anyhow::anyhow;
use anyhow::bail;
use blob::Blob;
use clidispatch::ReqCtx;
use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::fallback;
use cmdutil::FormatterOpts;
use cmdutil::IO;
use cmdutil::Result;
use cmdutil::WalkOpts;
use cmdutil::define_flags;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::FsNodeMetadata;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use pathmatcher::IntersectMatcher;
use repo::CoreRepo;
use storemodel::FileStore;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;
use vfs::UpdateFlag;
use vfs::VFS;
use vfs::Work;

define_flags! {
    pub struct CatOpts {
        /// print output to file with formatted name
        #[short('o')]
        #[argtype("FORMAT")]
        output: Option<String>,

        /// print the given revision
        #[short('r')]
        #[argtype("REV")]
        rev: String,

        walk_opts: WalkOpts,
        formatter_opts: FormatterOpts,

        #[args]
        args: Vec<String>,
    }
}

const BATCH_SIZE: usize = 1000;
const CONCURRENT_FETCHES: usize = 5;
const VFS_WORKERS: usize = 16;
const VFS_QUEUE_SIZE: usize = 1000;

/// FirstError helps propagate the first error seen in parallel operations. It also provides a
/// "has_error" method to aid in cancellation.
struct FirstError {
    tx: flume::Sender<anyhow::Error>,
    rx: flume::Receiver<anyhow::Error>,
    has_error: Arc<AtomicBool>,
}

impl Clone for FirstError {
    fn clone(&self) -> Self {
        FirstError {
            tx: self.tx.clone(),
            rx: self.rx.clone(),
            has_error: self.has_error.clone(),
        }
    }
}

impl FirstError {
    fn new() -> Self {
        let (tx, rx) = flume::bounded(1);
        FirstError {
            tx,
            rx,
            has_error: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Store error (if first).
    fn send_error(&self, err: anyhow::Error) {
        self.has_error.store(true, Ordering::Relaxed);
        let _ = self.tx.try_send(err);
    }

    /// Return whether an error has been stored. Useful for cancellation.
    fn has_error(&self) -> bool {
        self.has_error.load(Ordering::Relaxed)
    }

    /// Wait for all copies this FirstError to be dropped, and then yield the first error, if any.
    fn wait(self) -> anyhow::Result<()> {
        drop(self.tx);
        match self.rx.try_recv() {
            Ok(err) => Err(err),
            Err(_) => Ok(()),
        }
    }
}

enum Outputter {
    Disk {
        output_template: String,
        commit_id: HgId,
        repo_name: Option<String>,
        work_tx: flume::Sender<Work>,
        vfs_result_rx: flume::Receiver<Result<Work, (Option<Work>, anyhow::Error)>>,
    },
    Io(Arc<Mutex<IO>>),
}

impl Clone for Outputter {
    fn clone(&self) -> Self {
        match self {
            Outputter::Disk {
                output_template,
                commit_id,
                repo_name,
                work_tx,
                vfs_result_rx,
            } => Outputter::Disk {
                output_template: output_template.clone(),
                commit_id: *commit_id,
                repo_name: repo_name.clone(),
                work_tx: work_tx.clone(),
                vfs_result_rx: vfs_result_rx.clone(),
            },
            Outputter::Io(io) => Outputter::Io(io.clone()),
        }
    }
}

impl Outputter {
    fn new_disk(
        vfs: &VFS,
        output_template: String,
        commit_id: HgId,
        repo_name: Option<String>,
    ) -> Self {
        let (work_tx, vfs_result_rx) = vfs.batch(VFS_WORKERS, VFS_QUEUE_SIZE);
        Outputter::Disk {
            output_template,
            commit_id,
            repo_name,
            work_tx,
            vfs_result_rx,
        }
    }

    fn new_io(io: IO) -> Self {
        Outputter::Io(Arc::new(Mutex::new(io)))
    }

    fn fs_results(&self) -> Option<flume::Receiver<Result<Work, (Option<Work>, anyhow::Error)>>> {
        match self {
            Outputter::Disk { vfs_result_rx, .. } => Some(vfs_result_rx.clone()),
            Outputter::Io(_) => None,
        }
    }

    fn output_file(&self, path: &RepoPath, data: Blob, file_type: FileType) -> anyhow::Result<()> {
        match self {
            Outputter::Disk {
                output_template,
                commit_id,
                repo_name,
                work_tx,
                ..
            } => {
                let update_flag = match file_type {
                    FileType::Regular => UpdateFlag::Regular,
                    FileType::Executable => UpdateFlag::Executable,
                    FileType::Symlink => UpdateFlag::Symlink,
                    FileType::GitSubmodule => return Ok(()),
                };

                let filename =
                    make_output_filename(output_template, commit_id, path, repo_name.as_deref())?;
                let repo_path = RepoPathBuf::from_string(filename)?;
                work_tx
                    .send(Work::Write(repo_path, data, update_flag, None))
                    .map_err(|_| anyhow!("vfs worker channel closed"))?;
                Ok(())
            }
            Outputter::Io(io) => {
                let io = io.lock();
                let mut out = io.output();
                data.each_chunk(|chunk| out.write_all(chunk))?;
                Ok(())
            }
        }
    }
}

pub fn run(ctx: ReqCtx<CatOpts>, repo: &CoreRepo) -> Result<u8> {
    if matches!(repo, CoreRepo::Disk(_)) {
        // For now fall back to Python impl for normal use.
        fallback!("normal repo");
    }

    abort_if!(
        !ctx.opts.formatter_opts.template.is_empty(),
        "--template not supported"
    );

    let output_template = match &ctx.opts.output {
        Some(t) if t == "-" => None,
        Some(t) if t.is_empty() => abort!("--output cannot be empty"),
        Some(t) => Some(t.to_string()),
        None => None,
    };

    let matcher = pathmatcher::cli_matcher(
        &ctx.opts.args,
        &ctx.opts.walk_opts.include,
        &ctx.opts.walk_opts.exclude,
        pathmatcher::PatternKind::RelPath,
        true,
        "".as_ref(),
        "".as_ref(),
        &mut ctx.io().input(),
    )?;

    let mut matcher: DynMatcher = Arc::new(matcher);

    let commit_id = repo.resolve_commit(&ctx.opts.rev)?;

    let tree_resolver = repo.tree_resolver()?;
    let manifest = tree_resolver.get(&commit_id)?;
    let file_store = repo.file_store()?;

    // Check for sparse profile and intersect with existing matcher if set.
    if let Some(sparse_matcher) = repo.sparse_matcher(&manifest)? {
        matcher = Arc::new(IntersectMatcher::new(vec![matcher, sparse_matcher]));
    }

    let outputter = if let Some(output_template) = output_template {
        let repo_name = repo.repo_name().map(|s| s.to_string());

        // Split output template into prefix and relative template.
        let (prefix, relative_template) = split_output_template(&output_template)?;
        let vfs_path = std::env::current_dir()?.join(prefix);

        create_dir_all(&vfs_path)?;
        let vfs = VFS::new(vfs_path)?;

        Outputter::new_disk(&vfs, relative_template, commit_id, repo_name)
    } else {
        ctx.maybe_start_pager(repo.config())?;
        Outputter::new_io(ctx.io().clone())
    };

    let count = fetch_and_output(&manifest, matcher, &file_store, outputter)?;

    Ok(if count > 0 { 0 } else { 1 })
}

fn fetch_and_output<M: 'static + pathmatcher::Matcher + Sync + Send>(
    manifest: &TreeManifest,
    matcher: M,
    file_store: &Arc<dyn FileStore>,
    outputter: Outputter,
) -> Result<usize> {
    let first_error = FirstError::new();
    let output_count = Arc::new(AtomicUsize::new(0));

    let file_node_rx = manifest.iter(matcher);

    let (fetch_content_tx, fetch_content_rx) =
        flume::bounded::<Vec<(RepoPathBuf, FileMetadata)>>(CONCURRENT_FETCHES);

    let mut handles: Vec<thread::JoinHandle<()>> = Vec::new();

    // Spawn VFS error forwarder thread (only for Disk output)
    if let Some(rx) = outputter.fs_results() {
        let first_error = first_error.clone();
        handles.push(thread::spawn(move || {
            while let Ok(result) = rx.recv() {
                if let Err((_, err)) = result {
                    first_error.send_error(err);
                    break;
                }
            }
        }));
    }

    // Spawn fetch threads
    for _ in 0..CONCURRENT_FETCHES {
        let fetch_content_rx = fetch_content_rx.clone();
        let file_store = file_store.clone();
        let outputter = outputter.clone();
        let first_error = first_error.clone();
        let output_count = output_count.clone();

        handles.push(thread::spawn(move || {
            let run = || -> anyhow::Result<()> {
                while let Ok(batch) = fetch_content_rx.recv() {
                    if first_error.has_error() {
                        return Ok(());
                    }

                    let keys: Vec<Key> = batch
                        .iter()
                        .map(|(path, meta)| Key::new(path.clone(), meta.hgid))
                        .collect();

                    let iter =
                        file_store.get_content_iter(FetchContext::sapling_default(), keys)?;

                    let file_info = batch
                        .into_iter()
                        .map(|(path, meta)| (Key::new(path, meta.hgid), meta.file_type))
                        .collect::<HashMap<_, _>>();

                    for result in iter {
                        if first_error.has_error() {
                            return Ok(());
                        }

                        let (key, data) = result?;

                        let file_type = file_info
                            .get(&key)
                            .ok_or_else(|| anyhow!("missing file info for {}", key.hgid))?;

                        outputter.output_file(&key.path, data, *file_type)?;
                        output_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Ok(())
            };

            if let Err(e) = run() {
                first_error.send_error(e);
            }
        }));
    }

    drop(fetch_content_rx);
    drop(outputter);

    let mut current_batch = Vec::new();

    loop {
        if first_error.has_error() {
            break;
        }

        match file_node_rx.recv() {
            Ok(result_batch) => {
                for result in result_batch {
                    let (path, metadata) = match result {
                        Ok(v) => v,
                        Err(e) => {
                            first_error.send_error(e);
                            break;
                        }
                    };
                    if let FsNodeMetadata::File(file_meta) = metadata {
                        current_batch.push((path, file_meta));

                        if current_batch.len() >= BATCH_SIZE {
                            if fetch_content_tx
                                .send(std::mem::take(&mut current_batch))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // Channel disconnected, flush remaining batch
                if !current_batch.is_empty() {
                    let _ = fetch_content_tx.send(current_batch);
                }
                break;
            }
        }
    }

    drop(fetch_content_tx);

    // Wait for all threads
    for handle in handles {
        if let Err(e) = handle.join() {
            std::panic::resume_unwind(e);
        }
    }

    first_error.wait()?;

    Ok(output_count.load(Ordering::Relaxed))
}

/// Check if a string contains a format specifier (% followed by non-%).
fn has_formatter(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some('%') => {
                    chars.next();
                }
                _ => return true,
            }
        }
    }
    false
}

/// Split an output template into a prefix path and relative template.
///
/// Iterates through path components, accumulating a prefix until a component
/// with a format specifier is found. Returns the prefix (with %% collapsed to %)
/// and the remaining template (preserving format specifiers).
fn split_output_template(template: &str) -> Result<(PathBuf, String)> {
    use std::path::Component;

    let path = Path::new(template);

    let mut split_idx = 0;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        // Safety: path starts as a &str, so guaranteed UTF-8
        let component_str = component.as_os_str().to_str().unwrap();

        if has_formatter(component_str) {
            match component {
                Component::Prefix(_) | Component::RootDir => {
                    bail!("format specifier in path prefix is not supported");
                }
                _ => break,
            }
        }

        if components.peek().is_some() {
            split_idx += 1;
        }
    }

    let (prefix, suffix): (PathBuf, PathBuf) = path.components().partition(|_| {
        if split_idx == 0 {
            false
        } else {
            split_idx -= 1;
            true
        }
    });

    Ok((
        prefix.to_str().unwrap().replace("%%", "%").into(),
        suffix.to_str().unwrap().to_string(),
    ))
}

/// Expand format specifiers in the output template.
///
/// Supported format specifiers:
/// - `%%` - literal "%" character
/// - `%s` - basename of file being printed
/// - `%d` - dirname of file being printed, or '.' if in repository root
/// - `%p` - root-relative path name of file being printed
/// - `%H` - commit hash (40 hexadecimal digits)
/// - `%h` - short-form changeset hash (12 hexadecimal digits)
/// - `%b` - repository name
fn make_output_filename(
    template: &str,
    commit_id: &HgId,
    path: &RepoPath,
    repo_name: Option<&str>,
) -> Result<String> {
    let basename = path
        .last_component()
        .ok_or_else(|| anyhow!("invalid empty file name"))?
        .as_str();
    let dirname = path.parent().map(|p| p.as_str()).unwrap_or(".");

    let mut result = String::new();
    let mut chars = template.chars();

    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => result.push('%'),
                Some('s') => result.push_str(basename),
                Some('d') => result.push_str(dirname),
                Some('p') => result.push_str(path.as_str()),
                Some('H') => result.push_str(&commit_id.to_hex()),
                Some('h') => result.push_str(&commit_id.to_hex()[..12]),
                Some('b') => match repo_name {
                    Some(name) => result.push_str(name),
                    None => {
                        abort!("%b cannot be used without a repository name");
                    }
                },
                Some(other) => {
                    abort!("invalid formatter '%{}' in --output", other);
                }
                None => {
                    abort!("incomplete --output format - trailing '%'");
                }
            }
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

pub fn aliases() -> &'static str {
    "cat"
}

pub fn doc() -> &'static str {
    r#"output the current or given revision of files

    Print the specified files as they were at the given revision. If
    no revision is given, the parent of the working directory is used.

    Output may be to a file, in which case the name of the file is
    given using a format string. The formatting rules as follows:

    :``%%``: literal "%" character
    :``%s``: basename of file being printed
    :``%d``: dirname of file being printed, or '.' if in repository root
    :``%p``: root-relative path name of file being printed
    :``%H``: changeset hash (40 hexadecimal digits)
    :``%R``: changeset revision number
    :``%h``: short-form changeset hash (12 hexadecimal digits)
    :``%r``: zero-padded changeset revision number
    :``%b``: basename of the exporting repository

    Returns 0 on success.
    "#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[OPTION]... FILE...")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_formatter() {
        assert!(!has_formatter("foo"));
        assert!(!has_formatter("foo%%bar"));
        assert!(!has_formatter("%%"));
        assert!(!has_formatter(""));

        assert!(has_formatter("%s"));
        assert!(has_formatter("foo%s"));
        assert!(has_formatter("%sbar"));
        assert!(has_formatter("foo%sbar"));
        assert!(has_formatter("%%s%s"));
        assert!(has_formatter("%"));
    }

    #[test]
    fn test_split_output_template_relative() {
        // Simple relative path with formatter
        let (prefix, suffix) = split_output_template("output/%s").unwrap();
        assert_eq!(prefix, PathBuf::from("output"));
        assert_eq!(suffix, "%s");

        // Multiple path components before formatter
        let (prefix, suffix) = split_output_template("a/b/c/%p").unwrap();
        assert_eq!(prefix, PathBuf::from("a/b/c"));
        assert_eq!(suffix, "%p");

        // Formatter at start
        let (prefix, suffix) = split_output_template("%s").unwrap();
        assert_eq!(prefix, PathBuf::from(""));
        assert_eq!(suffix, "%s");

        // Formatter in middle component with trailing components
        let (prefix, suffix) = split_output_template("a/%s/b").unwrap();
        assert_eq!(prefix, PathBuf::from("a"));
        assert_eq!(suffix, if cfg!(windows) { r"%s\b" } else { "%s/b" });

        // No formatter - ensure we have at least one suffix component
        let (prefix, suffix) = split_output_template("a/b/c").unwrap();
        assert_eq!(prefix, PathBuf::from("a/b"));
        assert_eq!(suffix, "c");
        let (prefix, suffix) = split_output_template("a").unwrap();
        assert_eq!(prefix, PathBuf::new());
        assert_eq!(suffix, "a");
    }

    #[test]
    fn test_split_output_template_absolute() {
        // Absolute path with formatter
        let (prefix, suffix) = split_output_template("/tmp/output/%s").unwrap();
        assert_eq!(prefix, PathBuf::from("/tmp/output"));
        assert_eq!(suffix, "%s");

        // Formatter right after root
        let (prefix, suffix) = split_output_template("/%s").unwrap();
        assert_eq!(prefix, PathBuf::from("/"));
        assert_eq!(suffix, "%s");

        // No formatter - need at least one suffix component
        let (prefix, suffix) = split_output_template("/tmp/output").unwrap();
        assert_eq!(prefix, PathBuf::from("/tmp"));
        assert_eq!(suffix, "output");
        let (prefix, suffix) = split_output_template("/tmp").unwrap();
        assert_eq!(prefix, PathBuf::from("/"));
        assert_eq!(suffix, "tmp");

        #[cfg(windows)]
        {
            // Can't put formatter in path prefix
            assert!(split_output_template(r"\\he%bllo\foo\bar").is_err());

            // Can put a literal % in path prefix
            let (prefix, suffix) = split_output_template(r"\\he%%llo\foo\bar").unwrap();
            assert_eq!(prefix, PathBuf::from(r"\\he%llo\foo"));
            assert_eq!(suffix, "bar");
        }
    }

    #[test]
    fn test_split_output_template_escaped_percent() {
        // Escaped %% should collapse to % in prefix
        let (prefix, suffix) = split_output_template("foo%%bar/%s").unwrap();
        assert_eq!(prefix, PathBuf::from("foo%bar"));
        assert_eq!(suffix, "%s");

        // Multiple escaped %%
        let (prefix, suffix) = split_output_template("a%%b%%c/%s").unwrap();
        assert_eq!(prefix, PathBuf::from("a%b%c"));
        assert_eq!(suffix, "%s");

        // Single element - needs to be suffix
        let (prefix, suffix) = split_output_template("foo%%bar").unwrap();
        assert_eq!(prefix, PathBuf::new());
        assert_eq!(suffix, "foo%%bar");

        // %% followed by real formatter in same component
        let (prefix, suffix) = split_output_template("foo/%%%s").unwrap();
        assert_eq!(prefix, PathBuf::from("foo"));
        assert_eq!(suffix, "%%%s");
    }
}
