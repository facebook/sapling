/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::anyhow;
use clidispatch::ReqCtx;
use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::fallback;
use cmdutil::FormatterOpts;
use cmdutil::Result;
use cmdutil::WalkOpts;
use cmdutil::define_flags;
use manifest::Manifest;
use repo::CoreRepo;
use types::FetchContext;
use types::HgId;
use types::RepoPath;

define_flags! {
    pub struct CatOpts {
        /// print output to file with formatted name
        #[short('o')]
        #[argtype("FORMAT")]
        output: String,

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

pub fn run(ctx: ReqCtx<CatOpts>, repo: &CoreRepo) -> Result<u8> {
    if matches!(repo, CoreRepo::Disk(_)) {
        // For now fall back to Python impl for normal use.
        fallback!("normal repo");
    }

    abort_if!(
        !ctx.opts.formatter_opts.template.is_empty(),
        "--template not supported"
    );

    let output_template = &ctx.opts.output;
    let use_output_file = !output_template.is_empty() && output_template != "-";

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

    // TODO: support tenting

    let commit_id = repo.resolve_commit(&ctx.opts.rev)?;

    let tree_resolver = repo.tree_resolver()?;
    let manifest = tree_resolver.get(&commit_id)?;

    let file_store = repo.file_store()?;

    // Get repo name for %b format specifier
    let repo_name = repo.repo_name();

    let mut out = ctx.io().output();
    let mut saw_file = false;
    for file in manifest.files(matcher) {
        saw_file = true;
        let file = file?;
        let content =
            file_store.get_content(FetchContext::sapling_default(), &file.path, file.meta.hgid)?;

        if use_output_file {
            let filename =
                make_output_filename(output_template, &commit_id, &file.path, repo_name)?;

            // Create parent directories if needed
            if let Some(parent) = Path::new(&filename).parent() {
                let _ = fs::create_dir_all(parent);
            }

            let mut output_file = File::create(&filename)?;
            content.each_chunk(|chunk| output_file.write_all(chunk))?;
        } else {
            content.each_chunk(|chunk| out.write_all(chunk))?;
        }
    }

    Ok(if saw_file { 0 } else { 1 })
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
