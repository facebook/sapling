/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "fb")]
mod biggrep;

use std::borrow::Cow;
use std::cell::OnceCell;
use std::io;
use std::io::BufWriter;
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
use configmodel::Config;
use configmodel::Text;
use filewalk::walk_and_fetch;
use grep::matcher::Match;
use grep::matcher::Matcher;
use grep::regex::RegexMatcher;
use grep::regex::RegexMatcherBuilder;
use grep::searcher::BinaryDetection;
use grep::searcher::Searcher;
use grep::searcher::SearcherBuilder;
use grep::searcher::Sink;
use grep::searcher::SinkContext;
use grep::searcher::SinkFinish;
use grep::searcher::SinkMatch;
use pathmatcher::DynMatcher;
use pathmatcher::IntersectMatcher;
use repo::CoreRepo;
use serde::Serialize;
use termstyle::Styler;
use types::RepoPathBuf;
use types::hgid::WDIR_ID;
use types::path::RelativizedRepoPath;
use types::path::RepoPathRelativizer;

const OUTPUT_BUFFER_CAPACITY: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) struct GrepTextStyles {
    path: Text,
    line_number: Text,
    matched: Text,
}

impl GrepTextStyles {
    fn from_config(config: &dyn Config) -> Self {
        Self {
            path: config.get("color", "grep.path").unwrap_or_default(),
            line_number: config.get("color", "grep.line_number").unwrap_or_default(),
            matched: config.get("color", "grep.match").unwrap_or_default(),
        }
    }
}

#[derive(Default)]
struct RenderedStyle {
    prefix: Vec<u8>,
    suffix: Vec<u8>,
}

impl RenderedStyle {
    fn render(spec: &str, styler: &mut Styler) -> io::Result<Self> {
        let Some(rendered) = styler
            .render_style(spec)
            .map_err(|e| io::Error::other(e.to_string()))?
        else {
            return Ok(Self::default());
        };

        Ok(Self {
            prefix: rendered.prefix().to_vec(),
            suffix: rendered.suffix().to_vec(),
        })
    }

    fn is_empty(&self) -> bool {
        self.prefix.is_empty()
    }
}

pub(crate) struct PlainTextWriter<W: Write> {
    inner: BufWriter<W>,
    path_style: RenderedStyle,
    line_number_style: RenderedStyle,
    match_style: RenderedStyle,
}

impl<W: Write> PlainTextWriter<W> {
    fn new(inner: W, use_color: bool, styles: &GrepTextStyles) -> io::Result<Self> {
        let (path_style, line_number_style, match_style) = if use_color {
            let mut styler = Styler::new().map_err(|e| io::Error::other(e.to_string()))?;
            (
                RenderedStyle::render(&styles.path, &mut styler)?,
                RenderedStyle::render(&styles.line_number, &mut styler)?,
                RenderedStyle::render(&styles.matched, &mut styler)?,
            )
        } else {
            Default::default()
        };

        Ok(Self {
            inner: BufWriter::with_capacity(OUTPUT_BUFFER_CAPACITY, inner),
            path_style,
            line_number_style,
            match_style,
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn trim_line_terminator<'a>(&self, line: &'a [u8]) -> &'a [u8] {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        line.strip_suffix(b"\r").unwrap_or(line)
    }

    fn write_styled_bytes(
        inner: &mut BufWriter<W>,
        style: &RenderedStyle,
        bytes: &[u8],
    ) -> io::Result<()> {
        inner.write_all(&style.prefix)?;
        inner.write_all(bytes)?;
        inner.write_all(&style.suffix)
    }

    fn write_path(&mut self, path: impl std::fmt::Display) -> io::Result<()> {
        self.inner.write_all(&self.path_style.prefix)?;
        write!(self.inner, "{path}")?;
        self.inner.write_all(&self.path_style.suffix)
    }

    fn write_line_number(&mut self, line_number: u64) -> io::Result<()> {
        let line_number = line_number.to_string();
        Self::write_styled_bytes(
            &mut self.inner,
            &self.line_number_style,
            line_number.as_bytes(),
        )
    }

    fn write_prefix(
        &mut self,
        path: impl std::fmt::Display,
        line_number: Option<u64>,
        sep: u8,
    ) -> io::Result<()> {
        self.write_path(path)?;
        self.inner.write_all(&[sep])?;
        if let Some(line_number) = line_number {
            self.write_line_number(line_number)?;
            self.inner.write_all(&[sep])?;
        }
        Ok(())
    }

    fn write_matches(&mut self, line: &[u8], matches: &[Match]) -> io::Result<()> {
        let mut written = 0;
        for matched in matches
            .iter()
            .copied()
            .filter(|matched| !matched.is_empty())
        {
            if matched.start() > written {
                self.inner.write_all(&line[written..matched.start()])?;
            }
            Self::write_styled_bytes(&mut self.inner, &self.match_style, &line[matched])?;
            written = matched.end();
        }
        if written < line.len() {
            self.inner.write_all(&line[written..])?;
        }
        Ok(())
    }

    pub(crate) fn write_match_line(
        &mut self,
        path: impl std::fmt::Display,
        line_number: Option<u64>,
        line: &[u8],
        matches: &[Match],
    ) -> io::Result<()> {
        let line = self.trim_line_terminator(line);
        self.write_prefix(path, line_number, b':')?;
        if matches.is_empty() {
            self.inner.write_all(line)?;
        } else {
            self.write_matches(line, matches)?;
        }
        self.inner.write_all(b"\n")
    }

    pub(crate) fn write_plain_match_line(
        &mut self,
        path: impl std::fmt::Display,
        line_number: Option<u64>,
        line: &[u8],
    ) -> io::Result<()> {
        let line = self.trim_line_terminator(line);
        self.write_prefix(path, line_number, b':')?;
        self.inner.write_all(line)?;
        self.inner.write_all(b"\n")
    }

    pub(crate) fn write_context_line(
        &mut self,
        path: impl std::fmt::Display,
        line_number: Option<u64>,
        line: &[u8],
    ) -> io::Result<()> {
        let line = self.trim_line_terminator(line);
        self.write_prefix(path, line_number, b'-')?;
        self.inner.write_all(line)?;
        self.inner.write_all(b"\n")
    }

    pub(crate) fn write_context_break(&mut self) -> io::Result<()> {
        self.inner.write_all(b"--\n")
    }

    pub(crate) fn write_raw_line(&mut self, line: &[u8]) -> io::Result<()> {
        self.inner.write_all(line)?;
        self.inner.write_all(b"\n")
    }

    pub(crate) fn write_file_match(&mut self, path: impl std::fmt::Display) -> io::Result<()> {
        self.write_path(path)?;
        self.inner.write_all(b"\n")
    }

    pub(crate) fn write_binary_match(&mut self, path: impl std::fmt::Display) -> io::Result<()> {
        self.inner.write_all(b"Binary file ")?;
        self.write_path(path)?;
        self.inner.write_all(b" matches\n")
    }

    pub(crate) fn write_binary_quit_warning(
        &mut self,
        path: impl std::fmt::Display,
        quit_byte: u8,
        offset: u64,
    ) -> io::Result<()> {
        self.write_path(path)?;
        self.inner.write_all(b": ")?;
        let escaped = std::ascii::escape_default(quit_byte)
            .map(char::from)
            .collect::<String>();
        let remainder = format!(
            "WARNING: stopped searching binary file after match (found \"{escaped}\" byte around offset {offset})\n"
        );
        self.inner.write_all(remainder.as_bytes())
    }
}

/// A grep match entry for JSON output.
#[derive(Serialize)]
pub(crate) struct GrepMatch<'a, P: Serialize> {
    pub path: P,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_number: Option<u64>,
    pub text: &'a str,
}

/// A file match entry for JSON output (used with -l flag).
#[derive(Serialize)]
pub(crate) struct GrepFileMatch<P: Serialize> {
    pub path: P,
}

struct RelativizeOnce<'a> {
    relativizer: &'a RepoPathRelativizer,
    path: &'a RepoPathBuf,
    display: OnceCell<RelativizedRepoPath<'a>>,
}

impl<'a> RelativizeOnce<'a> {
    fn new(relativizer: &'a RepoPathRelativizer, path: &'a RepoPathBuf) -> Self {
        Self {
            relativizer,
            path,
            display: OnceCell::new(),
        }
    }

    fn display(&self) -> RelativizedRepoPath<'a> {
        *self
            .display
            .get_or_init(|| self.relativizer.relativized(self.path.as_repo_path()))
    }
}

/// A grepper that searches file contents for matches using the plain text printer.
pub(crate) struct Grepper<'a, W: Write> {
    regex_matcher: RegexMatcher,
    searcher: Searcher,
    relativizer: &'a RepoPathRelativizer,
    writer: PlainTextWriter<W>,
    invert_match: bool,
    match_count: u64,
}

/// A grepper for files_with_matches (-l) mode using the plain text printer.
pub(crate) struct SummaryGrepper<'a, W: Write> {
    regex_matcher: RegexMatcher,
    searcher: Searcher,
    relativizer: &'a RepoPathRelativizer,
    writer: PlainTextWriter<W>,
    match_count: u64,
}

impl<'a, W: Write> Grepper<'a, W> {
    /// Build a Grepper from GrepOpts, a pattern, relativizer, and output writer.
    pub(crate) fn new(
        opts: &GrepOpts,
        pattern: &str,
        relativizer: &'a RepoPathRelativizer,
        out: W,
        use_color: bool,
        styles: &GrepTextStyles,
    ) -> Result<Self> {
        let regex_matcher = build_regex_matcher(opts, pattern)?;
        let searcher = build_searcher(opts)?;

        Ok(Self {
            regex_matcher,
            searcher,
            relativizer,
            writer: PlainTextWriter::new(out, use_color, styles)?,
            invert_match: opts.invert_match,
            match_count: 0,
        })
    }

    /// Grep a file's contents for matches.
    pub(crate) fn grep_file(&mut self, path: &RepoPathBuf, data: &Blob) -> io::Result<()> {
        let display_path = RelativizeOnce::new(self.relativizer, path);

        data.each_chunk(|chunk| {
            let mut sink = StandardLineSink::new(
                &self.regex_matcher,
                &mut self.writer,
                &display_path,
                self.invert_match,
            );
            self.searcher
                .search_slice(&self.regex_matcher, chunk, &mut sink)
                .map_err(|e| io::Error::other(e.to_string()))?;
            self.match_count += sink.match_count;
            Ok(())
        })
    }

    /// Get the total number of matches found.
    pub(crate) fn match_count(&self) -> u64 {
        self.match_count
    }

    pub(crate) fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl<'a, W: Write> SummaryGrepper<'a, W> {
    /// Build a SummaryGrepper for files_with_matches mode.
    pub(crate) fn new(
        opts: &GrepOpts,
        pattern: &str,
        relativizer: &'a RepoPathRelativizer,
        out: W,
        use_color: bool,
        styles: &GrepTextStyles,
    ) -> Result<Self> {
        let regex_matcher = build_regex_matcher(opts, pattern)?;
        let searcher = build_searcher(opts)?;

        Ok(Self {
            regex_matcher,
            searcher,
            relativizer,
            writer: PlainTextWriter::new(out, use_color, styles)?,
            match_count: 0,
        })
    }

    /// Grep a file's contents for matches.
    pub(crate) fn grep_file(&mut self, path: &RepoPathBuf, data: &Blob) -> io::Result<()> {
        let display_path = RelativizeOnce::new(self.relativizer, path);
        let mut file_has_match = false;

        data.each_chunk(|chunk| {
            if file_has_match {
                return Ok(());
            }
            let mut sink = FileMatchLineSink::new(&mut self.writer, &display_path);
            self.searcher
                .search_slice(&self.regex_matcher, chunk, &mut sink)
                .map_err(|e| io::Error::other(e.to_string()))?;
            file_has_match = sink.has_match;
            Ok(())
        })?;
        if file_has_match {
            self.match_count += 1;
        }
        Ok(())
    }

    /// Get the total number of files with matches.
    pub(crate) fn match_count(&self) -> u64 {
        self.match_count
    }

    pub(crate) fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

struct StandardLineSink<'a, 'w, W: Write> {
    regex_matcher: &'a RegexMatcher,
    writer: &'w mut PlainTextWriter<W>,
    path: &'a RelativizeOnce<'a>,
    invert_match: bool,
    match_count: u64,
    match_positions: Vec<Match>,
    binary_byte_offset: Option<u64>,
}

impl<'a, 'w, W: Write> StandardLineSink<'a, 'w, W> {
    fn new(
        regex_matcher: &'a RegexMatcher,
        writer: &'w mut PlainTextWriter<W>,
        path: &'a RelativizeOnce<'a>,
        invert_match: bool,
    ) -> Self {
        Self {
            regex_matcher,
            writer,
            path,
            invert_match,
            match_count: 0,
            match_positions: Vec::new(),
            binary_byte_offset: None,
        }
    }
}

impl<W: Write> Sink for StandardLineSink<'_, '_, W> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let line_number = mat.line_number();
        let line = mat.bytes();
        let path = self.path.display();

        if self.invert_match || self.writer.match_style.is_empty() {
            self.writer
                .write_plain_match_line(path, line_number, line)?;
        } else {
            let line = self.writer.trim_line_terminator(line);
            self.match_positions.clear();
            self.regex_matcher
                .find_iter(line, |matched| {
                    self.match_positions.push(matched);
                    true
                })
                .map_err(|e| io::Error::other(e.to_string()))?;
            self.writer
                .write_match_line(path, line_number, line, &self.match_positions)?;
        }

        self.match_count += 1;
        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        context: &SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        self.writer.write_context_line(
            self.path.display(),
            context.line_number(),
            context.bytes(),
        )?;
        Ok(true)
    }

    fn context_break(&mut self, _searcher: &Searcher) -> Result<bool, Self::Error> {
        self.writer.write_context_break()?;
        Ok(true)
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        self.binary_byte_offset = Some(binary_byte_offset);
        Ok(true)
    }

    fn finish(&mut self, searcher: &Searcher, _finish: &SinkFinish) -> Result<(), Self::Error> {
        if self.match_count == 0 {
            return Ok(());
        }
        if let (Some(binary_byte_offset), Some(quit_byte)) = (
            self.binary_byte_offset,
            searcher.binary_detection().quit_byte(),
        ) {
            self.writer.write_binary_quit_warning(
                self.path.display(),
                quit_byte,
                binary_byte_offset,
            )?;
        }
        Ok(())
    }
}

struct FileMatchLineSink<'a, 'w, W: Write> {
    writer: &'w mut PlainTextWriter<W>,
    path: &'a RelativizeOnce<'a>,
    has_match: bool,
}

impl<'a, 'w, W: Write> FileMatchLineSink<'a, 'w, W> {
    fn new(writer: &'w mut PlainTextWriter<W>, path: &'a RelativizeOnce<'a>) -> Self {
        Self {
            writer,
            path,
            has_match: false,
        }
    }
}

impl<W: Write> Sink for FileMatchLineSink<'_, '_, W> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, _mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        self.writer.write_file_match(self.path.display())?;
        self.has_match = true;
        Ok(false)
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        _binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        Ok(false)
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

    // Without -E, convert BRE-style \| to ERE | (alternation).
    // In GNU grep BRE, \| means alternation. In Rust regex, \| is literal
    // pipe. We only convert \| — NOT \( \) \{ \} because users overwhelmingly
    // use \( to search for literal parentheses (e.g. "functionName\("), which
    // already works correctly in Rust regex.
    let pattern = if !opts.extended_regexp && !opts.fixed_strings {
        convert_bre_alternation(pattern)
    } else {
        Cow::Borrowed(pattern)
    };

    match matcher_builder.build(&pattern) {
        Ok(m) => Ok(m),
        Err(e) => abort!("invalid grep pattern '{}': {:?}", pattern, e),
    }
}

/// Convert BRE-style `\|` alternation to ERE-style `|`.
///
/// In GNU grep BRE, `\|` means alternation while `|` is literal.
/// In Rust regex (and ERE), `|` means alternation and `\|` is literal.
/// This strips the backslash from `\|` so it becomes `|` (alternation).
/// `\\|` (escaped backslash followed by pipe) is left unchanged.
///
/// Returns `Cow::Borrowed` if no conversion is needed (no allocation).
fn convert_bre_alternation(pattern: &str) -> Cow<'_, str> {
    // Scan bytes directly — `\` (0x5C) and `|` (0x7C) are ASCII, so they
    // cannot appear as continuation bytes in valid UTF-8.
    let bytes = pattern.as_bytes();

    // First pass: find indexes of backslashes to remove (the `\` before `|`).
    let mut removals = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'|' {
                removals.push(i);
            }
            i += 2;
        } else {
            i += 1;
        }
    }

    if removals.is_empty() {
        return Cow::Borrowed(pattern);
    }

    // Second pass: copy bytes, skipping the marked backslashes.
    let mut result = Vec::with_capacity(bytes.len() - removals.len());
    let mut ri = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if ri < removals.len() && i == removals[ri] {
            ri += 1;
        } else {
            result.push(b);
        }
    }

    // We only removed ASCII backslash bytes (0x5C) that preceded ASCII pipe
    // bytes (0x7C), so valid UTF-8 in = valid UTF-8 out.
    Cow::Owned(String::from_utf8(result).expect("BRE conversion preserved valid UTF-8"))
}

/// Build a searcher from grep options.
fn build_searcher(opts: &GrepOpts) -> Result<Searcher> {
    let mut searcher_builder = SearcherBuilder::new();

    // Skip binary files (files containing NUL bytes).
    searcher_builder.binary_detection(BinaryDetection::quit(0));

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

        /// use external search indexes when available (ADVANCED)
        external: bool = true,

        /// include unknown (untracked) files in the search (wdir only) (EXPERIMENTAL)
        unknown: bool,

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
    if !repo.config().get_or("grep", "use-rust", || false)? && ctx.opts.external {
        abort_if!(
            ctx.opts.rev.is_some(),
            "--rev requires --config grep.use-rust=true"
        );

        fallback!("grep.use-rust");
    }

    let pattern = &ctx.opts.grep_pattern;
    let include_unknown = ctx.opts.unknown;

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
        CoreRepo::Slapi(_slapi_repo) => {
            abort_if!(
                include_unknown,
                "--unknown is not supported for repoless grep"
            );
            (
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
            )
        }
    };

    abort_if!(
        include_unknown && rev != "wdir",
        "--unknown is only supported for the working directory (wdir)"
    );

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

    let (_, manifest) = if include_unknown {
        match repo {
            CoreRepo::Disk(repo) => {
                let wc = repo.working_copy()?;
                let wc = wc.read();
                let manifest = wc.working_manifest(&ctx.core, matcher.clone(), true)?;
                (WDIR_ID, manifest)
            }
            _ => unreachable!(),
        }
    } else {
        repo.resolve_manifest(&ctx.core, rev, matcher.clone())?
    };

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
    if ctx.opts.external {
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
    }

    let file_store = repo.file_store()?;

    ctx.maybe_start_pager(repo.config())?;

    let (file_rx, first_error) = walk_and_fetch(manifest, matcher, &file_store);

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
    file_rx: flume::Receiver<Vec<filewalk::FileResult>>,
    first_error: &filewalk::FirstError,
    pattern: &str,
    use_color: bool,
) -> Result<u64> {
    run_standard_grep_with_writer(
        ctx,
        relativizer,
        file_rx,
        first_error,
        pattern,
        ctx.io().output(),
        use_color,
    )
}

pub(crate) fn run_standard_grep_with_writer<W: Write>(
    ctx: &ReqCtx<GrepOpts>,
    relativizer: &RepoPathRelativizer,
    file_rx: flume::Receiver<Vec<filewalk::FileResult>>,
    first_error: &filewalk::FirstError,
    pattern: &str,
    out: W,
    use_color: bool,
) -> Result<u64> {
    let styles = GrepTextStyles::from_config(ctx.config().as_ref());
    let mut grepper = Grepper::new(&ctx.opts, pattern, relativizer, out, use_color, &styles)?;

    for file_batch in file_rx {
        if first_error.has_error() {
            break;
        }

        for file_result in file_batch {
            if let Err(e) = grepper.grep_file(&file_result.path, &file_result.data) {
                first_error.send_error(e.into());
                break;
            }
        }

        if first_error.has_error() {
            break;
        }
    }

    grepper.flush()?;
    Ok(grepper.match_count())
}

/// Run grep with summary output (files_with_matches mode).
fn run_summary_grep(
    ctx: &ReqCtx<GrepOpts>,
    relativizer: &RepoPathRelativizer,
    file_rx: flume::Receiver<Vec<filewalk::FileResult>>,
    first_error: &filewalk::FirstError,
    pattern: &str,
    use_color: bool,
) -> Result<u64> {
    run_summary_grep_with_writer(
        ctx,
        relativizer,
        file_rx,
        first_error,
        pattern,
        ctx.io().output(),
        use_color,
    )
}

pub(crate) fn run_summary_grep_with_writer<W: Write>(
    ctx: &ReqCtx<GrepOpts>,
    relativizer: &RepoPathRelativizer,
    file_rx: flume::Receiver<Vec<filewalk::FileResult>>,
    first_error: &filewalk::FirstError,
    pattern: &str,
    out: W,
    use_color: bool,
) -> Result<u64> {
    let styles = GrepTextStyles::from_config(ctx.config().as_ref());
    let mut grepper =
        SummaryGrepper::new(&ctx.opts, pattern, relativizer, out, use_color, &styles)?;

    for file_batch in file_rx {
        if first_error.has_error() {
            break;
        }

        for file_result in file_batch {
            if let Err(e) = grepper.grep_file(&file_result.path, &file_result.data) {
                first_error.send_error(e.into());
                break;
            }
        }

        if first_error.has_error() {
            break;
        }
    }

    grepper.flush()?;
    Ok(grepper.match_count())
}

/// A writer that outputs JSON in either array format or JSON Lines format.
pub(crate) struct JsonOutput {
    writer: BufWriter<Box<dyn Write>>,
    jsonl: bool,
    first: bool,
}

impl JsonOutput {
    pub fn new(writer: Box<dyn Write>, jsonl: bool) -> Self {
        Self {
            writer: BufWriter::with_capacity(OUTPUT_BUFFER_CAPACITY, writer),
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
        self.writer.flush()?;
        Ok(())
    }
}

/// Core grep loop that processes files and writes JSON output.
/// Used by both regular grep and biggrep for local file searching.
pub(crate) fn grep_files_json(
    opts: &GrepOpts,
    pattern: &str,
    file_rx: flume::Receiver<Vec<filewalk::FileResult>>,
    first_error: &filewalk::FirstError,
    relativizer: &RepoPathRelativizer,
    json_out: &mut JsonOutput,
) -> Result<u64> {
    let regex_matcher = build_regex_matcher(opts, pattern)?;
    let mut searcher = build_searcher(opts)?;
    let include_line_number = opts.line_number;
    let files_only = opts.files_with_matches;
    let mut match_count: u64 = 0;

    for file_batch in file_rx {
        if first_error.has_error() {
            break;
        }

        for file_result in file_batch {
            let display_path = RelativizeOnce::new(relativizer, &file_result.path);

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
                        path: display_path.display(),
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

        if first_error.has_error() {
            break;
        }
    }

    Ok(match_count)
}

/// A sink that writes grep matches directly to JSON output.
pub(crate) struct JsonWriteSink<'a, 'p> {
    pub path: &'a RelativizeOnce<'p>,
    pub include_line_number: bool,
    pub json_out: &'a mut JsonOutput,
    pub match_count: &'a mut u64,
}

impl Sink for JsonWriteSink<'_, '_> {
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
            path: self.path.display(),
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

    Use ``--unknown`` to also search unknown (untracked) files. This is only
    supported for the working directory (wdir).

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

      - Search only Python files using ``-I`` (include)::

          @prog@ grep -I "**.py" "import os"

      - Exclude test files::

          @prog@ grep -X "**test**" "TODO"

    Use :prog:`help patterns` for more information on specifying file patterns.

    Returns 0 if a match is found, 1 if no match."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[OPTION]... PATTERN [FILE]...")
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use configmodel::Text;

    use super::PlainTextWriter;
    use crate::GrepTextStyles;

    #[derive(Clone, Debug, Default)]
    struct CountingWriter {
        writes: Arc<AtomicUsize>,
        bytes: Arc<AtomicUsize>,
    }

    impl io::Write for CountingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.writes.fetch_add(1, Ordering::Relaxed);
            self.bytes.fetch_add(buf.len(), Ordering::Relaxed);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn plain_text_writer_batches_small_writes() -> io::Result<()> {
        let inner = CountingWriter::default();
        let writes = inner.writes.clone();
        let bytes = inner.bytes.clone();
        let styles = GrepTextStyles {
            path: Text::from_static(""),
            line_number: Text::from_static(""),
            matched: Text::from_static(""),
        };
        let mut writer = PlainTextWriter::new(inner, false, &styles)?;

        writer.write_plain_match_line("path", None, b"hello")?;
        writer.write_plain_match_line("path", Some(2), b"world")?;

        assert_eq!(writes.load(Ordering::Relaxed), 0);

        writer.flush()?;

        assert_eq!(writes.load(Ordering::Relaxed), 1);
        assert_eq!(
            bytes.load(Ordering::Relaxed),
            b"path:hello\npath:2:world\n".len()
        );
        Ok(())
    }
}
