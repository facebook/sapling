/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod print;

use std::sync::Arc;

use anyhow::Result;
use clidispatch::fallback;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use configloader::configmodel::ConfigExt;
use print::PrintConfig;
use print::PrintConfigStatusTypes;
use repo::repo::Repo;
use status::needs_morestatus_extension;
use types::path::RepoPathRelativizer;
use workingcopy::workingcopy::WorkingCopy;

use super::get_formatter;
use crate::commands::FormatterOpts;
use crate::commands::WalkOpts;

define_flags! {
    pub struct StatusOpts {
        /// show status of all files
        #[short('A')]
        all: bool,

        /// show only modified files
        #[short('m')]
        modified: bool,

        /// show only added files
        #[short('a')]
        added: bool,

        /// show only removed files
        #[short('r')]
        removed: bool,

        /// show only deleted (but tracked) files
        #[short('d')]
        deleted: bool,

        /// show only files without changes
        #[short('c')]
        clean: bool,

        /// show only unknown (not tracked) files
        #[short('u')]
        unknown: bool,

        /// show only ignored files
        #[short('i')]
        ignored: bool,

        /// hide status prefix
        #[short('n')]
        no_status: bool,

        /// show the terse output (EXPERIMENTAL)
        #[short('t')]
        terse: String,

        /// show source of copied files
        #[short('C')]
        copies: bool,

        /// end filenames with NUL, for use with xargs
        #[short('0')]
        print0: bool,

        /// show difference from revision
        #[argtype("REV")]
        rev: Vec<String>,

        /// list the changed files of a revision
        #[argtype("REV")]
        change: String,

        /// show status relative to root
        root_relative: Option<bool>,

        walk_opts: WalkOpts,
        formatter_opts: FormatterOpts,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<StatusOpts>, repo: &mut Repo, wc: &mut WorkingCopy) -> Result<u8> {
    if !repo.config().get_or_default("status", "use-rust")? {
        tracing::debug!(target: "status_info", status_detail="use_rust_disabled");
        fallback!("status.use-rust=false");
    }

    let rev_check = ctx.opts.rev.is_empty() || (ctx.opts.rev.len() == 1 && ctx.opts.rev[0] == ".");

    if ctx.opts.all
        || !ctx.opts.change.is_empty()
        || !ctx.opts.terse.is_empty()
        || !rev_check
        || ctx.opts.clean
    {
        tracing::debug!(target: "status_info", status_detail="unsupported_args");
        fallback!("one or more unsupported options in Rust status");
    }

    if repo.storage_format().is_git() {
        tracing::debug!(target: "status_info", status_detail="git");
        fallback!("git format unsupported (submodules)");
    }

    if needs_morestatus_extension(repo.dot_hg_path(), wc.treestate().lock().parents().count()) {
        tracing::debug!(target: "status_info", status_detail="morestatus_needed");
        fallback!("morestatus functionality needed");
    }

    let cwd = std::env::current_dir()?;
    let lgr = ctx.logger();

    let matcher = match pathmatcher::cli_matcher(
        &ctx.opts.args,
        &ctx.opts.walk_opts.include,
        &ctx.opts.walk_opts.exclude,
        pathmatcher::PatternKind::RelPath,
        wc.vfs().case_sensitive(),
        wc.vfs().root(),
        &cwd,
        &mut ctx.io().input(),
    ) {
        Ok(matcher) => {
            for warning in matcher.warnings() {
                lgr.warn(format!("warning: {}", warning));
            }

            Arc::new(matcher)
        }
        Err(err) => match err.downcast_ref::<pathmatcher::Error>() {
            Some(pathmatcher::Error::UnsupportedPatternKind(_)) => {
                tracing::debug!(target: "status_info", status_detail="unsupported_pattern");
                fallback!("unsupported pattern");
            }
            _ => return Err(err),
        },
    };

    let StatusOpts {
        modified,
        added,
        removed,
        deleted,
        clean,
        unknown,
        ignored,
        ..
    } = ctx.opts;

    let status_types = if modified || added || removed || deleted || clean || unknown || ignored {
        PrintConfigStatusTypes {
            modified,
            added,
            removed,
            deleted,
            clean,
            unknown,
            ignored,
        }
    } else {
        PrintConfigStatusTypes {
            modified: true,
            added: true,
            removed: true,
            deleted: true,
            clean: false,
            unknown: !ctx.global_opts().quiet,
            ignored: false,
        }
    };
    let print_config = PrintConfig {
        status_types,
        no_status: ctx.opts.no_status,
        copies: ctx.opts.copies
            || repo
                .config()
                .get_or::<bool>("ui", "statuscopies", || false)?,
        endl: if ctx.opts.print0 { '\0' } else { '\n' },
        root_relative: ctx
            .opts
            .root_relative
            .unwrap_or_else(|| hgplain::is_plain(None)),
    };

    tracing::debug!(target: "status_info", status_mode="rust");

    let status = wc.status(matcher.clone(), ignored, repo.config(), &ctx.logger())?;

    // This should be passed the "full" matcher including
    // ignores, sparse, etc., but in practice probably doesn't
    // make a difference.
    let copymap = wc.copymap(matcher.clone())?.into_iter().collect();

    let relativizer = RepoPathRelativizer::new(cwd, repo.path());
    let formatter = get_formatter(
        repo.config(),
        "status",
        ctx.opts.formatter_opts.template.as_str(),
        ctx.global_opts(),
        Box::new(ctx.io().output()),
    )?;

    let lgr = ctx.logger();
    for invalid in status.invalid_path() {
        lgr.warn(format!(
            "skipping invalid filename: '{}'",
            util::utf8::escape_non_utf8(invalid)
        ));
    }

    for invalid in status.invalid_type() {
        lgr.warn(format!("{invalid}: invalid file type"));
    }

    // Give the user warnings if explicitly specified files are "bad".
    for file in matcher.exact_files() {
        match wc.vfs().metadata(file) {
            Ok(fs_meta) => {
                // Warn about invalid file type (but only if we didn't already warn).
                if !fs_meta.is_dir()
                    && !fs_meta.is_file()
                    && !fs_meta.is_symlink()
                    && !status.invalid_type().contains(file)
                {
                    lgr.warn(format!(
                        "{}: invalid file type",
                        relativizer.relativize(file)
                    ));
                }
            }
            Err(err) => {
                if !status.contains(file) {
                    lgr.warn(format!("{}: {err}", relativizer.relativize(file)));
                }
            }
        }
    }

    ctx.maybe_start_pager(repo.config())?;

    print::print_status(formatter, relativizer, &print_config, &status, &copymap)?;

    Ok(0)
}

pub fn aliases() -> &'static str {
    "status|st|sta|stat|statu"
}

pub fn doc() -> &'static str {
    r#"list files with pending changes

    Show status of files in the working copy using the following status
    indicators::

      M = modified
      A = added
      R = removed
      C = clean
      ! = missing (deleted by a non-@prog@ command, but still tracked)
      ? = not tracked
      I = ignored
        = origin of the previous file (with --copies)

    By default, shows files that have been modified, added, removed,
    deleted, or that are unknown (corresponding to the options ``-mardu``,
    respectively). Files that are unmodified, ignored, or the source of
    a copy/move operation are not listed.

    To control the exact statuses that are shown, specify the relevant
    flags (like ``-rd`` to show only files that are removed or deleted).
    Additionally, specify ``-q/--quiet`` to hide both unknown and ignored
    files.

    To show the status of specific files, provide a list of files to
    match. To include or exclude files using patterns or filesets, use
    ``-I`` or ``-X``.

    If ``--rev`` is specified and only one revision is given, it is used as
    the base revision. If two revisions are given, the differences between
    them are shown. The ``--change`` option can also be used as a shortcut
    to list the changed files of a revision from its first parent.

    .. note::

       :prog:`status` might appear to disagree with :prog:`diff` if permissions
       have changed or a merge has occurred, because the standard diff
       format does not report permission changes and :prog:`diff` only
       reports changes relative to one merge parent.

    .. container:: verbose

      The ``-t/--terse`` option abbreviates the output by showing only the directory
      name if all the files in it share the same status. The option takes an
      argument indicating the statuses to abbreviate: 'm' for 'modified', 'a'
      for 'added', 'r' for 'removed', 'd' for 'deleted', 'u' for 'unknown', 'i'
      for 'ignored' and 'c' for clean.

      It abbreviates only those statuses which are passed. Note that clean and
      ignored files are not displayed with ``--terse ic`` unless the ``-c/--clean``
      and ``-i/--ignored`` options are also used.

      The ``-v/--verbose`` option shows information when the repository is in an
      unfinished merge, shelve, rebase state, etc. You can have this behavior
      turned on by default by enabling the ``commands.status.verbose`` config option.

      You can skip displaying some of these states by setting
      ``commands.status.skipstates`` to one or more of: 'bisect', 'graft',
      'histedit', 'merge', 'rebase', or 'unshelve'.

      Examples:

      - show changes in the working directory relative to a
        commit::

          @prog@ status --rev 88a692db8

      - show changes in the working copy relative to the
        current directory (see :prog:`help patterns` for more information)::

          @prog@ status re:

      - show all changes including copies in a commit::

          @prog@ status --copies --change 88a692db8

      - get a NUL separated list of added files, suitable for xargs::

          @prog@ status -an0

      - show more information about the repository status, abbreviating
        added, removed, modified, deleted, and untracked paths::

          @prog@ status -v -t mardu

    Returns 0 on success."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[OPTION]... [FILE]...")
}
