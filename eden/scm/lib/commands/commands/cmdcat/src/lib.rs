/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::ReqCtx;
use clidispatch::abort_if;
use clidispatch::fallback;
use cmdutil::FormatterOpts;
use cmdutil::Result;
use cmdutil::WalkOpts;
use cmdutil::define_flags;
use manifest::Manifest;
use repo::CoreRepo;
use types::FetchContext;

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
    abort_if!(!ctx.opts.output.is_empty(), "--output not supported");

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

    let mut out = ctx.io().output();
    let mut saw_file = false;
    for file in manifest.files(matcher) {
        saw_file = true;
        let file = file?;
        let content =
            file_store.get_content(FetchContext::sapling_default(), &file.path, file.meta.hgid)?;
        content.each_chunk(|chunk| out.write_all(chunk))?;
    }

    Ok(if saw_file { 0 } else { 1 })
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
