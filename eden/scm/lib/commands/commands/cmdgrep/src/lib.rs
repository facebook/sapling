/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "fb")]
mod biggrep;

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use blob::Blob;
use clidispatch::ReqCtx;
use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::fallback;
use cmdutil::ConfigExt;
use cmdutil::Result;
use cmdutil::WalkOpts;
use cmdutil::define_flags;
use filewalk::walk_and_fetch;
use grep::printer::ColorSpecs;
use grep::printer::Standard;
use grep::printer::StandardBuilder;
use grep::printer::Summary;
use grep::printer::SummaryBuilder;
use grep::printer::SummaryKind;
use grep::regex::RegexMatcher;
use grep::regex::RegexMatcherBuilder;
use grep::searcher::Searcher;
use grep::searcher::SearcherBuilder;
use pathmatcher::DynMatcher;
use pathmatcher::IntersectMatcher;
use repo::CoreRepo;
use termcolor::Ansi;
use termcolor::NoColor;
use termcolor::WriteColor;
use types::RepoPathBuf;
use types::path::RepoPathRelativizer;

/// A grepper that searches file contents for matches using the Standard printer.
pub(crate) struct Grepper<'a, W: WriteColor> {
    regex_matcher: RegexMatcher,
    searcher: Searcher,
    relativizer: &'a RepoPathRelativizer,
    printer: Standard<W>,
    match_count: u64,
}

/// A grepper for files_with_matches (-l) mode using the Summary printer.
pub(crate) struct SummaryGrepper<'a, W: WriteColor> {
    regex_matcher: RegexMatcher,
    searcher: Searcher,
    relativizer: &'a RepoPathRelativizer,
    printer: Summary<W>,
    match_count: u64,
}

impl<'a, W: WriteColor> Grepper<'a, W> {
    /// Build a Grepper from GrepOpts, a pattern, relativizer, and output writer.
    pub(crate) fn new(
        opts: &GrepOpts,
        pattern: &str,
        relativizer: &'a RepoPathRelativizer,
        out: W,
        use_color: bool,
    ) -> Result<Self> {
        let regex_matcher = build_regex_matcher(opts, pattern)?;
        let searcher = build_searcher(opts)?;

        // Build the standard printer
        let mut builder = StandardBuilder::new();
        if use_color {
            builder.color_specs(ColorSpecs::default_with_color());
        }

        let printer = builder.build(out);

        Ok(Self {
            regex_matcher,
            searcher,
            relativizer,
            printer,
            match_count: 0,
        })
    }

    /// Grep a file's contents for matches.
    pub(crate) fn grep_file(&mut self, path: &RepoPathBuf, data: &Blob) -> io::Result<()> {
        let display_path = self.relativizer.relativize(path);

        data.each_chunk(|chunk| {
            let mut sink = self
                .printer
                .sink_with_path(&self.regex_matcher, display_path.as_str());
            self.searcher
                .search_slice(&self.regex_matcher, chunk, &mut sink)
                .map_err(|e| io::Error::other(e.to_string()))?;
            self.match_count += sink.match_count();
            Ok(())
        })
    }

    /// Get the total number of matches found.
    pub(crate) fn match_count(&self) -> u64 {
        self.match_count
    }
}

impl<'a, W: WriteColor> SummaryGrepper<'a, W> {
    /// Build a SummaryGrepper for files_with_matches mode.
    pub(crate) fn new(
        opts: &GrepOpts,
        pattern: &str,
        relativizer: &'a RepoPathRelativizer,
        out: W,
        use_color: bool,
    ) -> Result<Self> {
        let regex_matcher = build_regex_matcher(opts, pattern)?;
        let searcher = build_searcher(opts)?;

        // Build the summary printer for PathWithMatch mode
        let mut builder = SummaryBuilder::new();
        builder.kind(SummaryKind::PathWithMatch);
        if use_color {
            builder.color_specs(ColorSpecs::default_with_color());
        }

        let printer = builder.build(out);

        Ok(Self {
            regex_matcher,
            searcher,
            relativizer,
            printer,
            match_count: 0,
        })
    }

    /// Grep a file's contents for matches.
    pub(crate) fn grep_file(&mut self, path: &RepoPathBuf, data: &Blob) -> io::Result<()> {
        let display_path = self.relativizer.relativize(path);

        data.each_chunk(|chunk| {
            let mut sink = self
                .printer
                .sink_with_path(&self.regex_matcher, display_path.as_str());
            self.searcher
                .search_slice(&self.regex_matcher, chunk, &mut sink)
                .map_err(|e| io::Error::other(e.to_string()))?;
            if sink.has_match() {
                self.match_count += 1;
            }
            Ok(())
        })
    }

    /// Get the total number of files with matches.
    pub(crate) fn match_count(&self) -> u64 {
        self.match_count
    }
}

/// Build a regex matcher from grep options.
fn build_regex_matcher(opts: &GrepOpts, pattern: &str) -> Result<RegexMatcher> {
    let mut matcher_builder = RegexMatcherBuilder::new();

    // -i: ignore case when matching
    if opts.ignore_case {
        matcher_builder.case_insensitive(true);
    }

    // -w: match whole words only
    if opts.word_regexp {
        matcher_builder.word(true);
    }

    // -F: interpret pattern as fixed string
    if opts.fixed_strings {
        matcher_builder.fixed_strings(true);
    }

    match matcher_builder.build(pattern) {
        Ok(m) => Ok(m),
        Err(e) => abort!("invalid grep pattern '{}': {:?}", pattern, e),
    }
}

/// Build a searcher from grep options.
fn build_searcher(opts: &GrepOpts) -> Result<Searcher> {
    let mut searcher_builder = SearcherBuilder::new();

    // Set line numbers based on -n flag (default is true in grep-searcher, so we must explicitly set)
    searcher_builder.line_number(opts.line_number);

    // -V: select non-matching lines (invert match)
    if opts.invert_match {
        searcher_builder.invert_match(true);
    }

    // -A: print NUM lines of trailing context
    if let Some(after_context) = opts.after_context {
        let after_context = match after_context.try_into() {
            Ok(v) => v,
            Err(err) => abort!("invalid --after-context value '{}': {}", after_context, err),
        };
        searcher_builder.after_context(after_context);
    }

    // -B: print NUM lines of leading context
    if let Some(before_context) = opts.before_context {
        let before_context = match before_context.try_into() {
            Ok(v) => v,
            Err(err) => abort!(
                "invalid --before-context value '{}': {}",
                before_context,
                err
            ),
        };
        searcher_builder.before_context(before_context);
    }

    // -C: print NUM lines of output context (both before and after)
    if let Some(context) = opts.context {
        let context = match context.try_into() {
            Ok(v) => v,
            Err(err) => abort!("invalid --context value '{}': {}", context, err),
        };
        searcher_builder.before_context(context);
        searcher_builder.after_context(context);
    }

    Ok(searcher_builder.build())
}

define_flags! {
    pub struct GrepOpts {
        walk_opts: WalkOpts,

        /// print NUM lines of trailing context
        #[short('A')]
        #[argtype("NUM")]
        after_context: Option<i64>,

        /// print NUM lines of leading context
        #[short('B')]
        #[argtype("NUM")]
        before_context: Option<i64>,

        /// print NUM lines of output context
        #[short('C')]
        #[argtype("NUM")]
        context: Option<i64>,

        /// ignore case when matching
        #[short('i')]
        ignore_case: bool,

        /// print only filenames that match
        #[short('l')]
        files_with_matches: bool,

        /// print matching line numbers
        #[short('n')]
        line_number: bool,

        /// select non-matching lines
        #[short('V')]
        invert_match: bool,

        /// match whole words only
        #[short('w')]
        word_regexp: bool,

        /// use POSIX extended regexps
        #[short('E')]
        extended_regexp: bool,

        /// interpret pattern as fixed string
        #[short('F')]
        fixed_strings: bool,

        /// use Perl-compatible regexps
        #[short('P')]
        perl_regexp: bool,

        /// search the repository as it is at REV (ADVANCED)
        #[short('r')]
        #[argtype("REV")]
        rev: Option<String>,

        #[arg]
        grep_pattern: String,

        #[args]
        sl_patterns: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<GrepOpts>, repo: &CoreRepo) -> Result<u8> {
    if !repo.config().get_or("grep", "use-rust", || false)? {
        abort_if!(
            ctx.opts.rev.is_some(),
            "--rev requires --config grep.use-rust=true"
        );

        fallback!("grep.use-rust");
    }

    let pattern = &ctx.opts.grep_pattern;

    let (repo_root, case_sensitive, cwd, sl_patterns, relativizer, rev) = match repo {
        CoreRepo::Disk(repo) => {
            let wc = repo.working_copy()?;
            let wc = wc.read();
            let vfs = wc.vfs();
            (
                Some(vfs.root().to_path_buf()),
                vfs.case_sensitive(),
                std::env::current_dir()?,
                if ctx.opts.sl_patterns.is_empty() {
                    // Default to "." (i.e. search cwd).
                    &[".".to_string()][..]
                } else {
                    &ctx.opts.sl_patterns
                },
                RepoPathRelativizer::new(std::env::current_dir()?, vfs.root()),
                ctx.opts.rev.as_deref().unwrap_or("wdir"),
            )
        }
        CoreRepo::Slapi(_slapi_repo) => (
            None,
            true,
            PathBuf::new(),
            if ctx.opts.sl_patterns.is_empty() {
                abort!("FILE pattern(s) required in repoless mode");
            } else {
                ctx.opts.sl_patterns.as_slice()
            },
            RepoPathRelativizer::noop(),
            match ctx.opts.rev.as_deref() {
                Some(rev) => rev,
                None => abort!("--rev is required for repoless grep"),
            },
        ),
    };

    // For cli_matcher, use empty path if repo_root is None
    let cli_matcher_root = repo_root.as_deref().unwrap_or(Path::new(""));
    let hinted_matcher = pathmatcher::cli_matcher(
        sl_patterns,
        &ctx.opts.walk_opts.include,
        &ctx.opts.walk_opts.exclude,
        pathmatcher::PatternKind::RelPath,
        case_sensitive,
        cli_matcher_root,
        &cwd,
        &mut ctx.io().input(),
    )?;
    let matcher: DynMatcher = Arc::new(hinted_matcher.clone());

    let (_, manifest) = repo.resolve_manifest(&ctx.core, rev, matcher.clone())?;

    // Check for sparse profile and intersect with existing matcher if set.
    let matcher = if let Some(sparse_matcher) = repo.sparse_matcher(&manifest)? {
        Arc::new(IntersectMatcher::new(vec![matcher, sparse_matcher])) as DynMatcher
    } else {
        matcher
    };

    // Check if we should use biggrep (FB-only feature)
    #[cfg(feature = "fb")]
    if let Some(exit_code) = biggrep::try_biggrep(
        &ctx,
        repo,
        pattern,
        hinted_matcher.exact_files(),
        &matcher,
        &relativizer,
        &cwd,
        repo_root.as_deref(),
    )? {
        return Ok(exit_code);
    }

    let file_store = repo.file_store()?;

    ctx.maybe_start_pager(repo.config())?;

    let (file_rx, first_error) = walk_and_fetch(&manifest, matcher, &file_store);

    let use_color = ctx.should_color();

    // Use the appropriate grepper based on mode
    let match_count = if ctx.opts.files_with_matches {
        run_summary_grep(
            &ctx,
            &relativizer,
            file_rx,
            &first_error,
            pattern,
            use_color,
        )?
    } else {
        run_standard_grep(
            &ctx,
            &relativizer,
            file_rx,
            &first_error,
            pattern,
            use_color,
        )?
    };

    first_error.wait()?;

    Ok(if match_count > 0 { 0 } else { 1 })
}

/// Run grep with standard output (line-by-line matches).
fn run_standard_grep(
    ctx: &ReqCtx<GrepOpts>,
    relativizer: &RepoPathRelativizer,
    file_rx: flume::Receiver<filewalk::FileResult>,
    first_error: &filewalk::FirstError,
    pattern: &str,
    use_color: bool,
) -> Result<u64> {
    let out = ctx.io().output();

    if use_color {
        let ansi_out = Ansi::new(out);
        run_standard_grep_with_writer(
            ctx,
            relativizer,
            file_rx,
            first_error,
            pattern,
            ansi_out,
            true,
        )
    } else {
        let no_color_out = NoColor::new(out);
        run_standard_grep_with_writer(
            ctx,
            relativizer,
            file_rx,
            first_error,
            pattern,
            no_color_out,
            false,
        )
    }
}

fn run_standard_grep_with_writer<W: WriteColor>(
    ctx: &ReqCtx<GrepOpts>,
    relativizer: &RepoPathRelativizer,
    file_rx: flume::Receiver<filewalk::FileResult>,
    first_error: &filewalk::FirstError,
    pattern: &str,
    out: W,
    use_color: bool,
) -> Result<u64> {
    let mut grepper = Grepper::new(&ctx.opts, pattern, relativizer, out, use_color)?;

    for file_result in file_rx {
        if first_error.has_error() {
            break;
        }

        if let Err(e) = grepper.grep_file(&file_result.path, &file_result.data) {
            first_error.send_error(e.into());
            break;
        }
    }

    Ok(grepper.match_count())
}

/// Run grep with summary output (files_with_matches mode).
fn run_summary_grep(
    ctx: &ReqCtx<GrepOpts>,
    relativizer: &RepoPathRelativizer,
    file_rx: flume::Receiver<filewalk::FileResult>,
    first_error: &filewalk::FirstError,
    pattern: &str,
    use_color: bool,
) -> Result<u64> {
    let out = ctx.io().output();

    if use_color {
        let ansi_out = Ansi::new(out);
        run_summary_grep_with_writer(
            ctx,
            relativizer,
            file_rx,
            first_error,
            pattern,
            ansi_out,
            true,
        )
    } else {
        let no_color_out = NoColor::new(out);
        run_summary_grep_with_writer(
            ctx,
            relativizer,
            file_rx,
            first_error,
            pattern,
            no_color_out,
            false,
        )
    }
}

fn run_summary_grep_with_writer<W: WriteColor>(
    ctx: &ReqCtx<GrepOpts>,
    relativizer: &RepoPathRelativizer,
    file_rx: flume::Receiver<filewalk::FileResult>,
    first_error: &filewalk::FirstError,
    pattern: &str,
    out: W,
    use_color: bool,
) -> Result<u64> {
    let mut grepper = SummaryGrepper::new(&ctx.opts, pattern, relativizer, out, use_color)?;

    for file_result in file_rx {
        if first_error.has_error() {
            break;
        }

        if let Err(e) = grepper.grep_file(&file_result.path, &file_result.data) {
            first_error.send_error(e.into());
            break;
        }
    }

    Ok(grepper.match_count())
}

pub fn aliases() -> &'static str {
    "grep|gre"
}

pub fn doc() -> &'static str {
    r#"search for a pattern in tracked files in the working directory

    The default regexp style is POSIX basic regexps. If no FILE parameters are
    passed in, the current directory and its subdirectories will be searched.

    For the old '@prog@ grep', which searches through history, see 'histgrep'."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[OPTION]... PATTERN [FILE]...")
}
