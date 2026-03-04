/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "fb")]
mod biggrep;

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
use cmdutil::Result;
use cmdutil::WalkOpts;
use cmdutil::define_flags;
use filewalk::walk_and_fetch;
use grep::regex::RegexMatcher;
use grep::regex::RegexMatcherBuilder;
use grep::searcher::Searcher;
use grep::searcher::SearcherBuilder;
use grep::searcher::Sink;
use grep::searcher::SinkContext;
use grep::searcher::SinkMatch;
use pathmatcher::DynMatcher;
use pathmatcher::IntersectMatcher;
use repo::CoreRepo;
use types::RepoPathBuf;
use types::path::RepoPathRelativizer;

/// A grepper that searches file contents for matches.
pub(crate) struct Grepper<'a, W: Write> {
    regex_matcher: RegexMatcher,
    searcher: Searcher,
    relativizer: &'a RepoPathRelativizer,
    sink: GrepSink<W>,
}

/// Sink for grep operations that handles output formatting.
struct GrepSink<W: Write> {
    path: String,
    out: W,
    match_count: usize,
    file_matched: bool,
    files_with_matches: bool,
    print_line_number: bool,
}

impl<'a, W: Write> Grepper<'a, W> {
    /// Build a Grepper from GrepOpts, a pattern, relativizer, and output writer.
    pub(crate) fn new(
        opts: &GrepOpts,
        pattern: &str,
        relativizer: &'a RepoPathRelativizer,
        out: W,
    ) -> Result<Self> {
        // Build the regex matcher with appropriate options
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

        let regex_matcher = match matcher_builder.build(pattern) {
            Ok(m) => m,
            Err(e) => abort!("invalid grep pattern '{}': {:?}", pattern, e),
        };

        // Build the searcher with appropriate options
        let mut searcher_builder = SearcherBuilder::new();

        // -n: print matching line numbers (enabled on searcher so sink receives correct line numbers)
        if opts.line_number {
            searcher_builder.line_number(true);
        }

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

        let searcher = searcher_builder.build();

        Ok(Self {
            regex_matcher,
            searcher,
            relativizer,
            sink: GrepSink {
                path: String::new(),
                out,
                match_count: 0,
                file_matched: false,
                files_with_matches: opts.files_with_matches,
                print_line_number: opts.line_number,
            },
        })
    }

    /// Grep a file's contents for matches.
    pub(crate) fn grep_file(&mut self, path: &RepoPathBuf, data: &Blob) -> std::io::Result<()> {
        self.sink.path = self.relativizer.relativize(path);
        self.sink.file_matched = false;

        data.each_chunk(|chunk| {
            // For -l mode, stop searching this file once we have a match
            if self.sink.files_with_matches && self.sink.file_matched {
                return Ok(());
            }

            self.searcher
                .search_slice(&self.regex_matcher, chunk, &mut self.sink)
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            Ok(())
        })
    }

    /// Get the total number of matches found.
    pub(crate) fn match_count(&self) -> usize {
        self.sink.match_count
    }
}

impl<W: Write> Sink for GrepSink<W> {
    type Error = std::io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> std::result::Result<bool, Self::Error> {
        self.match_count += 1;

        // -l: print only filenames that match
        if self.files_with_matches {
            if !self.file_matched {
                self.file_matched = true;
                writeln!(self.out, "{}", self.path)?;
            }
            return Ok(true);
        }

        let line = String::from_utf8_lossy(mat.bytes());
        if self.print_line_number {
            if let Some(line_num) = mat.line_number() {
                write!(self.out, "{}:{}:{}", self.path, line_num, line)?;
            } else {
                write!(self.out, "{}:{}", self.path, line)?;
            }
        } else {
            write!(self.out, "{}:{}", self.path, line)?;
        }
        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        context: &SinkContext<'_>,
    ) -> std::result::Result<bool, Self::Error> {
        // Don't print context for files_with_matches mode
        if self.files_with_matches {
            return Ok(true);
        }

        let line = String::from_utf8_lossy(context.bytes());
        // Context lines use '-' separator instead of ':'
        if self.print_line_number {
            if let Some(line_num) = context.line_number() {
                write!(self.out, "{}-{}-{}", self.path, line_num, line)?;
            } else {
                write!(self.out, "{}-{}", self.path, line)?;
            }
        } else {
            write!(self.out, "{}-{}", self.path, line)?;
        }
        Ok(true)
    }

    fn context_break(&mut self, _searcher: &Searcher) -> std::result::Result<bool, Self::Error> {
        // Don't print context break for files_with_matches mode
        if self.files_with_matches {
            return Ok(true);
        }
        writeln!(self.out, "--")?;
        Ok(true)
    }
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

    // Build the grepper
    let mut grepper = Grepper::new(&ctx.opts, pattern, &relativizer, ctx.io().output())?;

    for file_result in file_rx {
        if first_error.has_error() {
            break;
        }

        if let Err(e) = grepper.grep_file(&file_result.path, &file_result.data) {
            first_error.send_error(e.into());
            break;
        }
    }

    first_error.wait()?;

    Ok(if grepper.match_count() > 0 { 0 } else { 1 })
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
