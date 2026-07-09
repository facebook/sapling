/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::create_dir_all;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
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
use filewalk::FileResult;
use filewalk::WalkInput;
use filewalk::WalkOptions;
use filewalk::walk_and_fetch;
use manifest::FileType;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use pathmatcher::IntersectMatcher;
use repo::CoreRepo;
use storemodel::FileStore;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;
use vfs::UpdateFlag;
use vfs::VFS;
use vfs::VfsBatchError;
use vfs::Work;

define_flags! {
    pub struct CatOpts {
        /// print output to file with formatted name
        #[short('o')]
        #[argtype("FORMAT")]
        output: Option<String>,

        /// write files and content to a tar archive
        tar: bool,

        /// replace binary files at or above BYTES with placeholder content (EXPERIMENTAL)
        #[argtype("BYTES")]
        binary_file_size_threshold: Option<i64>,

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

const VFS_WORKERS: usize = 16;

enum Outputter {
    Io {
        io: Arc<Mutex<IO>>,
        error: Option<anyhow::Error>,
    },
    Tar {
        output_template: String,
        commit_id: HgId,
        repo_name: Option<String>,
        builder: Arc<Mutex<tar::Builder<Box<dyn Write + Send>>>>,
        error: Option<anyhow::Error>,
    },
}

impl Outputter {
    fn new_io(io: IO) -> Self {
        Outputter::Io {
            io: Arc::new(Mutex::new(io)),
            error: None,
        }
    }

    fn new_tar(
        writer: impl Write + Send + 'static,
        output_template: String,
        commit_id: HgId,
        repo_name: Option<String>,
    ) -> Self {
        Outputter::Tar {
            output_template,
            commit_id,
            repo_name,
            builder: Arc::new(Mutex::new(tar::Builder::new(Box::new(writer)))),
            error: None,
        }
    }

    fn finish(self) -> anyhow::Result<()> {
        match self {
            Outputter::Io { io, error } => {
                let finish_result = io.lock().output().flush().map_err(Into::into);
                error.map_or(finish_result, Err)
            }
            Outputter::Tar { builder, error, .. } => {
                let finish_result = builder.lock().finish().map_err(Into::into);
                error.map_or(finish_result, Err)
            }
        }
    }

    fn set_error(&mut self, err: anyhow::Error) {
        match self {
            Outputter::Io { error, .. } | Outputter::Tar { error, .. } => {
                if error.is_none() {
                    *error = Some(err);
                }
            }
        }
    }

    fn output_file(
        &mut self,
        path: &RepoPath,
        hgid: HgId,
        data: Blob,
        file_type: FileType,
    ) -> bool {
        match self {
            Outputter::Io { io, error } => {
                let io = io.lock();
                let mut out = io.output();
                if let Err(err) = data.each_chunk(|chunk| out.write_all(chunk)) {
                    *error = Some(err.into());
                    return false;
                }
                true
            }
            Outputter::Tar {
                output_template,
                commit_id,
                repo_name,
                builder,
                error,
            } => {
                let tar_path = match make_output_filename(
                    output_template,
                    commit_id,
                    path,
                    repo_name.as_deref(),
                ) {
                    Ok(tar_path) => tar_path,
                    Err(err) => {
                        *error = Some(err);
                        return false;
                    }
                };
                let mut builder = builder.lock();
                if let Err(err) =
                    append_tar_entry(&mut builder, path, &tar_path, hgid, data, file_type)
                {
                    *error = Some(err);
                    return false;
                }
                true
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

    let output = match &ctx.opts.output {
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

    let (commit_id, manifest) = repo.resolve_manifest(&ctx.core, &ctx.opts.rev, matcher.clone())?;

    let file_store = repo.file_store()?;

    // Check for sparse profile and intersect with existing matcher if set.
    if let Some(sparse_matcher) = repo.sparse_matcher(&manifest)? {
        matcher = Arc::new(IntersectMatcher::new(vec![matcher, sparse_matcher]));
    }

    let binary_file_size_threshold = match ctx.opts.binary_file_size_threshold {
        Some(threshold) if threshold < 0 => {
            abort!("--binary-file-size-threshold must be non-negative");
        }
        Some(threshold) => Some(threshold as usize),
        None => None,
    };

    let count = if ctx.opts.tar {
        let output_template = output
            .filter(|t| t != "-")
            .unwrap_or_else(|| "%p".to_string());
        let repo_name = repo.repo_name().map(|s| s.to_string());
        fetch_and_output(
            manifest,
            matcher,
            &file_store,
            Outputter::new_tar(ctx.io().output(), output_template, commit_id, repo_name),
            binary_file_size_threshold,
            WalkOptions::from_config(repo.config())?,
        )?
    } else if let Some(output_template) = output.filter(|t| t != "-") {
        let repo_name = repo.repo_name().map(|s| s.to_string());

        // Split output template into prefix and relative template.
        let (prefix, relative_template) = split_output_template(&output_template)?;
        let vfs_path = std::env::current_dir()?.join(prefix);

        create_dir_all(&vfs_path)?;
        let vfs = VFS::new(vfs_path)?;

        fetch_and_output_disk(
            manifest,
            matcher,
            &file_store,
            &vfs,
            relative_template,
            commit_id,
            repo_name,
            binary_file_size_threshold,
            WalkOptions::from_config(repo.config())?,
        )?
    } else {
        ctx.maybe_start_pager(repo.config())?;
        fetch_and_output(
            manifest,
            matcher,
            &file_store,
            Outputter::new_io(ctx.io().clone()),
            binary_file_size_threshold,
            WalkOptions::from_config(repo.config())?,
        )?
    };

    Ok(if count > 0 { 0 } else { 1 })
}

fn fetch_and_output(
    manifest: TreeManifest,
    matcher: DynMatcher,
    file_store: &Arc<dyn FileStore>,
    mut outputter: Outputter,
    binary_file_size_threshold: Option<usize>,
    options: WalkOptions,
) -> Result<usize> {
    let file_items = walk_and_fetch(WalkInput::Manifest(manifest), matcher, file_store, options);
    let mut output_count = 0;

    'output: for file_batch in file_items.into_batches() {
        let file_batch = match file_batch {
            Ok(file_batch) => file_batch,
            Err(err) => {
                outputter.set_error(err);
                break 'output;
            }
        };
        for file_result in file_batch {
            let FileResult {
                path,
                hgid,
                data,
                file_type,
            } = file_result;
            let (data, file_type) = match filter_binary_file(
                &path,
                hgid,
                data,
                file_type,
                binary_file_size_threshold,
            ) {
                Ok(file) => file,
                Err(err) => {
                    outputter.set_error(err);
                    break 'output;
                }
            };
            if !outputter.output_file(&path, hgid, data, file_type) {
                break 'output;
            }
            output_count += 1;
        }
    }

    outputter.finish()?;
    Ok(output_count)
}

fn fetch_and_output_disk(
    manifest: TreeManifest,
    matcher: DynMatcher,
    file_store: &Arc<dyn FileStore>,
    vfs: &VFS,
    output_template: String,
    commit_id: HgId,
    repo_name: Option<String>,
    binary_file_size_threshold: Option<usize>,
    options: WalkOptions,
) -> Result<usize> {
    let file_items = walk_and_fetch(WalkInput::Manifest(manifest), matcher, file_store, options);
    let work_items = file_items
        .map_batch(|batch| batch.map_err(VfsBatchError::Batch))
        .try_map_item(move |file_result| {
            file_result_to_work(
                &output_template,
                commit_id,
                repo_name.as_deref(),
                file_result,
                binary_file_size_threshold,
            )
        })
        .map_batch(|batch| Ok(batch?.into_iter().flatten().collect::<Vec<_>>()));

    let mut output_count = 0;
    let mut first_error = None;
    for result_batch in vfs.batch_items(VFS_WORKERS, work_items).into_batches() {
        match result_batch {
            Ok(batch) => output_count += batch.len(),
            Err(err) => {
                if first_error.is_none() {
                    first_error = Some(err.into_error());
                }
            }
        }
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(output_count),
    }
}

fn file_result_to_work(
    output_template: &str,
    commit_id: HgId,
    repo_name: Option<&str>,
    file_result: FileResult,
    binary_file_size_threshold: Option<usize>,
) -> std::result::Result<Option<Work>, VfsBatchError> {
    let FileResult {
        path,
        hgid,
        data,
        file_type,
    } = file_result;
    let (data, file_type) =
        filter_binary_file(&path, hgid, data, file_type, binary_file_size_threshold)
            .map_err(VfsBatchError::Batch)?;
    let update_flag = match file_type {
        FileType::Regular => UpdateFlag::Regular,
        FileType::Executable => UpdateFlag::Executable,
        FileType::Symlink => UpdateFlag::Symlink,
        FileType::GitSubmodule => return Ok(None),
    };
    let filename = make_output_filename(output_template, &commit_id, &path, repo_name)
        .map_err(VfsBatchError::Batch)?;
    let repo_path =
        RepoPathBuf::from_string(filename).map_err(|err| VfsBatchError::Batch(err.into()))?;
    Ok(Some(Work::Write(repo_path, data, update_flag, None)))
}

fn filter_binary_file(
    path: &RepoPath,
    hgid: HgId,
    data: Blob,
    file_type: FileType,
    threshold: Option<usize>,
) -> anyhow::Result<(Blob, FileType)> {
    let Some(threshold) = threshold else {
        return Ok((data, file_type));
    };

    if data.len() < threshold {
        return Ok((data, file_type));
    }

    if blob_contains_nul(&data)? {
        let placeholder = format!(
            "This is a placeholder for a large binary file\n\nOriginal file path: {}\nOriginal file id: {}\nOriginal file size: {}\n",
            path.as_str(),
            hgid.to_hex(),
            data.len(),
        );
        Ok((Blob::from(placeholder.into_bytes()), FileType::Regular))
    } else {
        Ok((data, file_type))
    }
}

fn blob_contains_nul(data: &Blob) -> std::io::Result<bool> {
    let mut contains_nul = false;
    let result = data.each_chunk(|chunk| {
        if chunk.contains(&0) {
            contains_nul = true;
            Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "found NUL byte",
            ))
        } else {
            Ok(())
        }
    });

    if contains_nul {
        Ok(true)
    } else {
        result.map(|()| false)
    }
}

fn append_tar_entry(
    builder: &mut tar::Builder<Box<dyn Write + Send>>,
    path: &RepoPath,
    tar_path: &str,
    hgid: HgId,
    data: Blob,
    file_type: FileType,
) -> anyhow::Result<()> {
    match file_type {
        FileType::Regular | FileType::Executable => {
            let size = data.len();
            let mut data = data.into_reader();
            let mut header = tar::Header::new_gnu();
            header.set_size(size as u64);
            header.set_mode(if file_type == FileType::Executable {
                0o755
            } else {
                0o644
            });
            builder.append_data(&mut header, Path::new(tar_path), &mut data)?;
        }
        FileType::Symlink => {
            let target = data.into_vec();
            let target = std::str::from_utf8(&target).with_context(|| {
                format!(
                    "invalid UTF-8 symlink target for {} (file id {}): {:?}",
                    path.as_str(),
                    hgid.to_hex(),
                    target,
                )
            })?;
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            builder.append_link(&mut header, Path::new(tar_path), Path::new(target))?;
        }
        FileType::GitSubmodule => {}
    }
    Ok(())
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
    r#"output file content at a particular revision

    Output the specified files' content at the specified revision. If
    no revision is given, the parent of the working directory is used.

    Use ``--output`` to write files or directories to disk using the following
    formatting rules:

    :``%%``: literal "%" character
    :``%s``: basename of file being printed
    :``%d``: dirname of file being printed, or '.' if in repository root
    :``%p``: root-relative path name of file being printed
    :``%H``: commit hash (40 hexadecimal digits)
    :``%h``: short commit hash (12 hexadecimal digits)
    :``%b``: basename of the repository

    .. container:: verbose

      Examples:

      - Recursively export directory foo/bar to disk::

          @prog@ cat -r fbc6b8c381 --output "/tmp/export/%p" path:foo/bar

      - Output all Rust files' content under foo/bar to stdout::

          @prog@ cat -r fbc6b8c381 "glob:foo/bar/**/*.rs"

      - Output the content of something/important.txt at bookmark main to /tmp/file::

          @prog@ cat -r main --output /tmp/file something/important.txt

    To operate without a local repo, specify ``-R/--repository`` as a Sapling
    Remote API capable URL. The local on-disk cache will still be used to avoid
    remote fetches.

    See :prog:`help patterns` for more information on specifying file patterns.

    Returns 0 if there were no errors and at least one file was output.
    "#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[-r REV] [-o OUTFILESPEC] PATTERN...")
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
