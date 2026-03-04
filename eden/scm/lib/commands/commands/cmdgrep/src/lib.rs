/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "fb")]
mod biggrep;

use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use blob::Blob;
use clidispatch::ReqCtx;
use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::fallback;
use cmdutil::ConfigExt;
use cmdutil::FormatterOpts;
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
use grep::searcher::Sink;
use grep::searcher::SinkMatch;
use pathmatcher::DynMatcher;
use pathmatcher::IntersectMatcher;
use repo::CoreRepo;
use serde::Serialize;
use termcolor::Ansi;
use termcolor::NoColor;
use termcolor::WriteColor;
use types::RepoPathBuf;
use types::path::RepoPathRelativizer;

/// A grep match entry for JSON output.
#[derive(Serialize)]
pub(crate) struct GrepMatch<'a> {
    pub path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_number: Option<u64>,
    pub text: &'a str,
}

/// A file match entry for JSON output (used with -l flag).
#[derive(Serialize)]
pub(crate) struct GrepFileMatch<'a> {
    pub path: &'a str,
}

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

        formatter_opts: FormatterOpts,

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

    // Check for JSON templates early and validate unsupported flag combinations
    let template = ctx.opts.formatter_opts.template.as_str();
    let is_json_template = matches!(template, "json" | "jsonl");

    if is_json_template {
        abort_if!(
            ctx.opts.after_context.is_some(),
            "-A/--after-context is not supported with -T {}",
            template
        );
        abort_if!(
            ctx.opts.before_context.is_some(),
            "-B/--before-context is not supported with -T {}",
            template
        );
        abort_if!(
            ctx.opts.context.is_some(),
            "-C/--context is not supported with -T {}",
            template
        );
    }

    // Validate template before proceeding
    if !matches!(template, "" | "json" | "jsonl") {
        abort!("unknown template: {}", template);
    }

    // Create JsonOutput if needed (shared between biggrep and local grep)
    let mut json_out = if is_json_template {
        Some(JsonOutput::new(
            Box::new(ctx.io().output()),
            template == "jsonl",
        ))
    } else {
        None
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
        json_out.as_mut(),
    )? {
        if let Some(json_out) = json_out {
            json_out.finish()?;
        }
        return Ok(exit_code);
    }

    let file_store = repo.file_store()?;

    ctx.maybe_start_pager(repo.config())?;

    let (file_rx, first_error) = walk_and_fetch(&manifest, matcher, &file_store);

    // Handle JSON output modes
    if let Some(mut json_out) = json_out {
        grep_files_json(
            &ctx.opts,
            pattern,
            file_rx,
            &first_error,
            &relativizer,
            &mut json_out,
        )?;
        first_error.wait()?;
        json_out.finish()?;
        return Ok(0);
    }

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

/// A writer that outputs JSON in either array format or JSON Lines format.
pub(crate) struct JsonOutput {
    writer: Box<dyn Write>,
    jsonl: bool,
    first: bool,
}

impl JsonOutput {
    pub fn new(writer: Box<dyn Write>, jsonl: bool) -> Self {
        Self {
            writer,
            jsonl,
            first: true,
        }
    }

    pub fn write<T: Serialize>(&mut self, value: &T) -> io::Result<()> {
        if self.jsonl {
            serde_json::to_writer(&mut self.writer, value)
                .map_err(|e| io::Error::other(e.to_string()))?;
            writeln!(self.writer)?;
        } else {
            if self.first {
                write!(self.writer, "[\n  ")?;
                self.first = false;
            } else {
                write!(self.writer, ",\n  ")?;
            }
            serde_json::to_writer(&mut self.writer, value)
                .map_err(|e| io::Error::other(e.to_string()))?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<()> {
        if !self.jsonl {
            if self.first {
                // No items written
                writeln!(self.writer, "[]")?;
            } else {
                writeln!(self.writer, "\n]")?;
            }
        }
        Ok(())
    }
}

/// Core grep loop that processes files and writes JSON output.
/// Used by both regular grep and biggrep for local file searching.
pub(crate) fn grep_files_json(
    opts: &GrepOpts,
    pattern: &str,
    file_rx: flume::Receiver<filewalk::FileResult>,
    first_error: &filewalk::FirstError,
    relativizer: &RepoPathRelativizer,
    json_out: &mut JsonOutput,
) -> Result<u64> {
    let regex_matcher = build_regex_matcher(opts, pattern)?;
    let mut searcher = build_searcher(opts)?;
    let include_line_number = opts.line_number;
    let files_only = opts.files_with_matches;
    let mut match_count: u64 = 0;

    for file_result in file_rx {
        if first_error.has_error() {
            break;
        }

        let display_path = relativizer.relativize(&file_result.path);

        if files_only {
            // In files_only mode, just check if file has any match
            let mut file_has_match = false;
            if let Err(e) = file_result.data.each_chunk(|chunk| {
                let mut sink = FileMatchSink {
                    has_match: &mut file_has_match,
                };
                searcher
                    .search_slice(&regex_matcher, chunk, &mut sink)
                    .map_err(|e| io::Error::other(e.to_string()))?;
                Ok(())
            }) {
                first_error.send_error(e.into());
                break;
            }
            if file_has_match {
                json_out.write(&GrepFileMatch {
                    path: &display_path,
                })?;
                match_count += 1;
            }
        } else {
            // Output matches directly as we find them
            if let Err(e) = file_result.data.each_chunk(|chunk| {
                let mut sink = JsonWriteSink {
                    path: &display_path,
                    include_line_number,
                    json_out,
                    match_count: &mut match_count,
                };
                searcher
                    .search_slice(&regex_matcher, chunk, &mut sink)
                    .map_err(|e| io::Error::other(e.to_string()))?;
                Ok(())
            }) {
                first_error.send_error(e.into());
                break;
            }
        }
    }

    Ok(match_count)
}

/// A sink that writes grep matches directly to JSON output.
pub(crate) struct JsonWriteSink<'a> {
    pub path: &'a str,
    pub include_line_number: bool,
    pub json_out: &'a mut JsonOutput,
    pub match_count: &'a mut u64,
}

impl Sink for JsonWriteSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let line_number = if self.include_line_number {
            mat.line_number()
        } else {
            None
        };

        // Get the matched text, trimming the trailing newline
        let text = String::from_utf8_lossy(mat.bytes());
        let text = text.trim_end_matches('\n');

        self.json_out.write(&GrepMatch {
            path: self.path,
            line_number,
            text,
        })?;
        *self.match_count += 1;

        Ok(true)
    }
}

/// A sink that just tracks whether any match was found.
pub(crate) struct FileMatchSink<'a> {
    pub has_match: &'a mut bool,
}

impl Sink for FileMatchSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, _mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        *self.has_match = true;
        // Return false to stop searching after first match
        Ok(false)
    }
}

pub fn aliases() -> &'static str {
    "grep|gre"
}

pub fn doc() -> &'static str {
    r#"search for a pattern in tracked files

    Search tracked files in the working directory for a regular expression
    pattern. If no FILE patterns are given, searches the current directory
    recursively.

    Use ``-r/--rev REV`` to search files at a specific revision instead of the
    working directory. ``--rev`` defaults to "wdir", which includes uncommitted
    changes to tracked files.

    To operate without a local repo, specify ``-R/--repository`` as a Sapling
    Remote API capable URL. The local on-disk cache will still be used to avoid
    remote fetches.

    .. container:: verbose

      Examples:

      - Search for "TODO" recursively in the current directory::

          @prog@ grep TODO

      - Search for a pattern in specific files, showing context on both sides::

          @prog@ grep -C 3 "pub fn .* -> String" "glob:**/*.rs"

      - Search at a specific revision::

          @prog@ grep -r main "my_function" path:my/project

      - Case-insensitive search under lib/, showing line numbers::

          @prog@ grep -in "error" lib

      - List files containing matches::

          @prog@ grep -l "deprecated"

    Returns 0 if a match is found, 1 if no match."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[OPTION]... PATTERN [FILE]...")
}
