/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! BigGrep integration for the grep command.
//!
//! This module provides integration with BigGrep, an external search index
//! that can speed up grep operations in large repositories.

use std::collections::HashSet;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;

use clidispatch::ReqCtx;
use clidispatch::abort;
use cmdutil::ConfigExt;
use cmdutil::Result;
use manifest::DiffType;
use manifest::Manifest;
use pathmatcher::DynMatcher;
use pathmatcher::ExactMatcher;
use pathmatcher::IntersectMatcher;
use pathmatcher::Matcher;
use repo::CoreRepo;
use types::RepoPath;
use types::RepoPathBuf;
use types::path::RepoPathRelativizer;

use crate::GrepFileMatch;
use crate::GrepMatch;
use crate::GrepOpts;
use crate::GrepTextStyles;
use crate::JsonOutput;
use crate::PlainTextWriter;
use crate::run_standard_grep_with_writer;
use crate::run_summary_grep_with_writer;

/// Check if biggrep should be used and run it if so.
/// Returns Some(exit_code) if biggrep was used, None if we should fall back to local grep.
///
/// Parameters:
/// - `exact_files`: Exact file paths to scope biggrep search (from HintedMatcher)
/// - `matcher`: Used for filtering results (includes sparse profile intersection)
/// - `json_out`: If Some, output will be JSON format; if None, plain text output
pub fn try_biggrep(
    ctx: &ReqCtx<GrepOpts>,
    repo: &CoreRepo,
    pattern: &str,
    exact_files: &[RepoPathBuf],
    matcher: &DynMatcher,
    relativizer: &RepoPathRelativizer,
    cwd: &Path,
    repo_root: Option<&Path>,
    json_out: Option<&mut JsonOutput>,
) -> Result<Option<u8>> {
    let config = repo.config();

    // Check if biggrep is explicitly enabled/disabled
    let use_biggrep = config.get_opt::<bool>("grep", "usebiggrep")?;
    // Track if biggrep was explicitly enabled (vs auto-detected) - affects error handling
    let explicitly_enabled = use_biggrep == Some(true);

    // Get biggrep configuration
    let Some(biggrep_client) = config.get_opt::<PathBuf>("grep", "biggrepclient")? else {
        return Ok(None);
    };
    let Some(biggrep_tier) = config.get("grep", "biggreptier") else {
        return Ok(None);
    };
    let Some(biggrep_corpus) = config.get_opt::<String>("grep", "biggrepcorpus")? else {
        return Ok(None);
    };

    // Determine if we should use biggrep
    let should_use_biggrep = match use_biggrep {
        Some(explicit) => explicit,
        None => {
            // Auto-enable if: corpus configured + client exists + (eden repo or SlapiRepo)
            let is_eden_or_slapi = match repo {
                CoreRepo::Disk(disk_repo) => disk_repo.requirements.contains("eden"),
                CoreRepo::Slapi(_) => true, // SlapiRepo is always remote, treat as eden-like
            };
            is_eden_or_slapi && Path::new(&biggrep_client).exists()
        }
    };

    if !should_use_biggrep {
        return Ok(None);
    }

    // -V (invert match) is not supported with biggrep
    if ctx.opts.invert_match {
        abort!("Cannot use invert_match option with biggrep");
    }

    // Build the biggrep pattern
    let biggrep_pattern = if ctx.opts.word_regexp {
        format!(r"\b{}\b", pattern)
    } else if ctx.opts.fixed_strings {
        pattern.to_string()
    } else {
        pattern.replace("-", r"\-")
    };

    // Choose search engine based on fixed_strings option
    let biggrep_engine = if ctx.opts.fixed_strings {
        "apr_strmatch"
    } else {
        "re2"
    };

    // Build the biggrep command
    let mut cmd = Command::new(&biggrep_client);
    cmd.arg(&*biggrep_tier)
        .arg(&biggrep_corpus)
        .arg(biggrep_engine)
        .arg("--stripdir")
        .arg("-r")
        .arg("--expression")
        .arg(&biggrep_pattern);

    // Add context options
    if let Some(after) = ctx.opts.after_context {
        cmd.arg("-A").arg(after.to_string());
    }
    if let Some(before) = ctx.opts.before_context {
        cmd.arg("-B").arg(before.to_string());
    }
    if let Some(context) = ctx.opts.context {
        cmd.arg("-C").arg(context.to_string());
    }
    if ctx.opts.ignore_case {
        cmd.arg("-i");
    }
    if ctx.opts.files_with_matches {
        cmd.arg("-l");
    }

    // Scope biggrep to the appropriate path
    if exact_files.is_empty() {
        // No explicit files - scope to cwd relative to repo root
        if let Some(rel_cwd) = repo_root.and_then(|r| cwd.strip_prefix(r).ok()) {
            if !rel_cwd.as_os_str().is_empty() {
                cmd.arg("-f").arg(rel_cwd);
            }
        }
    } else {
        // Scope to the patterns specified by the user.
        // Should we regex escape the file names? Python didn't - could be breaking change.
        let files: Vec<_> = exact_files.iter().map(|p| p.as_str()).collect();
        let pattern = format!("({})", files.join("|"));
        cmd.arg("-f").arg(pattern);
    }

    // Output debug info if requested
    if ctx.global_opts().debug {
        let cmd_args: Vec<_> = std::iter::once(cmd.get_program().to_string_lossy().into_owned())
            .chain(cmd.get_args().map(|a| a.to_string_lossy().into_owned()))
            .collect();
        ctx.logger()
            .info(format!("biggrep command: {:?}", cmd_args));
    }

    // Execute biggrep with streaming output
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            if explicitly_enabled {
                abort!(
                    "biggrep_client failed to start: {}\n(pass `--config grep.usebiggrep=False` to bypass biggrep)",
                    e
                );
            }
            return Ok(None);
        }
    };

    let stdout = child.stdout.take().expect("stdout should be captured");
    let stderr = child.stderr.take().expect("stderr should be captured");
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // First line contains the corpus revision info (starts with #)
    let first_line = match lines.next() {
        Some(Ok(line)) if line.starts_with('#') => line,
        Some(Ok(_line)) => {
            // First line doesn't start with #, unexpected format
            let status = child.wait()?;
            if explicitly_enabled {
                // Read stderr for error message
                let mut stderr_content = String::new();
                let _ = BufReader::new(stderr).read_to_string(&mut stderr_content);
                abort!(
                    "biggrep_client failed with exit code {}: {}\n(pass `--config grep.usebiggrep=False` to bypass biggrep)",
                    status.code().unwrap_or(-1),
                    stderr_content.trim()
                );
            }
            return Ok(None);
        }
        Some(Err(e)) => {
            let _ = child.wait();
            return Err(e.into());
        }
        None => {
            // No output from biggrep
            let status = child.wait()?;
            if explicitly_enabled {
                // Read stderr for error message
                let mut stderr_content = String::new();
                let _ = BufReader::new(stderr).read_to_string(&mut stderr_content);
                abort!(
                    "biggrep_client failed with exit code {}: {}\n(pass `--config grep.usebiggrep=False` to bypass biggrep)",
                    status.code().unwrap_or(-1),
                    stderr_content.trim()
                );
            }
            return Ok(None);
        }
    };

    // Parse the corpus revision and compute changed files before processing results
    let corpus_rev = parse_corpus_revision(&first_line[1..]);
    let target_rev = ctx.opts.rev.as_deref().unwrap_or("wdir");

    let (files_to_grep, files_to_exclude) = match corpus_rev.as_ref() {
        Some(corpus_rev) => {
            match compute_changed_files(ctx, repo, corpus_rev, target_rev, matcher) {
                Ok((to_grep, to_exclude)) => (to_grep, to_exclude),
                Err(e) => {
                    ctx.logger().warn(format!(
                        "Could not check for changes since corpus revision {}: {}. Results may be stale.",
                        corpus_rev, e
                    ));
                    (HashSet::new(), HashSet::new())
                }
            }
        }
        None => (HashSet::new(), HashSet::new()),
    };

    ctx.maybe_start_pager(config.as_ref())?;

    let files_with_matches = ctx.opts.files_with_matches;
    let include_line_number = ctx.opts.line_number;

    if let Some(json_out) = json_out {
        for line_result in lines {
            let line = line_result?;
            match parse_biggrep_line(
                &line,
                files_with_matches,
                matcher,
                &files_to_grep,
                &files_to_exclude,
            )? {
                BigGrepLine::Match {
                    repo_path,
                    lineno,
                    context,
                } => {
                    let rel_path = relativizer.relativized(repo_path.as_repo_path());
                    if files_with_matches {
                        json_out.write(&GrepFileMatch { path: &rel_path })?;
                    } else {
                        let line_number = if include_line_number {
                            lineno.and_then(|s| s.parse().ok())
                        } else {
                            None
                        };
                        json_out.write(&GrepMatch {
                            path: &rel_path,
                            line_number,
                            text: context.unwrap_or_default(),
                        })?;
                    }
                }
                _ => {}
            }
        }

        wait_for_biggrep(&mut child)?;

        if !files_to_grep.is_empty() {
            run_local_grep_json(
                ctx,
                repo,
                pattern,
                build_changed_files_matcher(matcher, &files_to_grep),
                relativizer,
                target_rev,
                json_out,
            )?;
        }

        Ok(Some(0))
    } else {
        let styles = GrepTextStyles::from_config(config.as_ref());
        let mut out = PlainTextWriter::new(ctx.io().output(), ctx.should_color(), &styles)?;
        let mut match_count: u64 = 0;

        for line_result in lines {
            let line = line_result?;
            match parse_biggrep_line(
                &line,
                files_with_matches,
                matcher,
                &files_to_grep,
                &files_to_exclude,
            )? {
                BigGrepLine::Match {
                    repo_path,
                    lineno,
                    context,
                } => {
                    let rel_path = relativizer.relativized(repo_path.as_repo_path());
                    if files_with_matches {
                        out.write_file_match(rel_path)?;
                    } else {
                        let line_number = if include_line_number {
                            lineno.and_then(|s| s.parse().ok())
                        } else {
                            None
                        };
                        out.write_plain_match_line(
                            rel_path,
                            line_number,
                            context.unwrap_or_default().as_bytes(),
                        )?;
                    }
                    match_count += 1;
                }
                BigGrepLine::BinaryMatch { repo_path } => {
                    let rel_path = relativizer.relativized(repo_path.as_repo_path());
                    out.write_binary_match(&rel_path)?;
                    match_count += 1;
                }
                BigGrepLine::Raw => {
                    out.write_raw_line(line.as_bytes())?;
                }
                BigGrepLine::Skip => {}
            }
        }

        wait_for_biggrep(&mut child)?;

        if !files_to_grep.is_empty() {
            out.flush()?;
            match_count += run_local_grep(
                ctx,
                repo,
                pattern,
                build_changed_files_matcher(matcher, &files_to_grep),
                relativizer,
                target_rev,
            )?;
        }

        out.flush()?;
        Ok(Some(if match_count > 0 { 0 } else { 1 }))
    }
}

enum BigGrepLine<'a> {
    Match {
        repo_path: RepoPathBuf,
        lineno: Option<&'a str>,
        context: Option<&'a str>,
    },
    BinaryMatch {
        repo_path: RepoPathBuf,
    },
    Raw,
    Skip,
}

/// Parse a raw biggrep output line into its components.
/// Returns None for unparsable lines.
fn parse_biggrep_fields(
    line: &str,
    files_with_matches: bool,
) -> Option<(&str, Option<&str>, Option<&str>, bool)> {
    if files_with_matches {
        return Some((line, None, None, false));
    }

    // Format: filename:lineno:colno:context
    if let Some((filename, rest)) = line.split_once(':') {
        if let Some((lineno, rest)) = rest.split_once(':') {
            if let Some((_colno, context)) = rest.split_once(':') {
                return Some((filename, Some(lineno), Some(context), false));
            }
        }
    }

    // Format: "Binary file X matches"
    if line.starts_with("Binary file ") && line.ends_with(" matches") {
        return Some((&line[12..line.len() - 8], None, None, true));
    }

    None
}

/// Parse and filter a biggrep output line.
fn parse_biggrep_line<'a>(
    line: &'a str,
    files_with_matches: bool,
    matcher: &DynMatcher,
    files_to_grep: &HashSet<RepoPathBuf>,
    files_to_exclude: &HashSet<RepoPathBuf>,
) -> Result<BigGrepLine<'a>> {
    if line.is_empty() {
        return Ok(BigGrepLine::Skip);
    }

    let Some((filename, lineno, context, is_binary)) =
        parse_biggrep_fields(line, files_with_matches)
    else {
        return Ok(BigGrepLine::Raw);
    };

    let plain_filename = strip_ansi_escapes(filename);
    let Ok(repo_path) = RepoPath::from_str(&plain_filename).map(|p| p.to_owned()) else {
        return Ok(BigGrepLine::Skip);
    };
    if !matcher.matches_file(&repo_path)?
        || files_to_grep.contains(&repo_path)
        || files_to_exclude.contains(&repo_path)
    {
        return Ok(BigGrepLine::Skip);
    }

    if is_binary {
        Ok(BigGrepLine::BinaryMatch { repo_path })
    } else {
        Ok(BigGrepLine::Match {
            repo_path,
            lineno,
            context,
        })
    }
}

fn wait_for_biggrep(child: &mut std::process::Child) -> Result<()> {
    let status = child.wait()?;
    if !matches!(status.code(), Some(0) | Some(1)) {
        abort!(
            "biggrep_client failed with exit code {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn build_changed_files_matcher(
    matcher: &DynMatcher,
    files_to_grep: &HashSet<RepoPathBuf>,
) -> DynMatcher {
    Arc::new(IntersectMatcher::new(vec![
        matcher.clone(),
        Arc::new(ExactMatcher::new(files_to_grep.iter(), true)),
    ]))
}

/// Compute what files have changed between the corpus revision and the target revision.
/// Returns (files_to_grep, files_to_exclude) where:
/// - files_to_grep: files that were added or modified (need local grep)
/// - files_to_exclude: files that were removed (skip biggrep results)
fn compute_changed_files(
    ctx: &ReqCtx<GrepOpts>,
    repo: &CoreRepo,
    corpus_rev: &str,
    target_rev: &str,
    matcher: &DynMatcher,
) -> Result<(HashSet<RepoPathBuf>, HashSet<RepoPathBuf>)> {
    // Get the corpus manifest
    let (_corpus_id, corpus_manifest) =
        repo.resolve_manifest(&ctx.core, corpus_rev, matcher.clone())?;

    // Get the target revision manifest
    let (_target_id, target_manifest) =
        repo.resolve_manifest(&ctx.core, target_rev, matcher.clone())?;

    let mut files_to_grep = HashSet::new();
    let mut files_to_exclude = HashSet::new();

    // Diff corpus (left) vs target (right)
    // LeftOnly = in corpus but not in target = removed (exclude)
    // RightOnly = in target but not in corpus = added (grep)
    // Changed = in both but different = modified (grep)
    for diff_result in corpus_manifest.diff(&target_manifest, matcher.clone())? {
        let entry = diff_result?;
        match entry.diff_type {
            DiffType::LeftOnly(_) => {
                files_to_exclude.insert(entry.path);
            }
            DiffType::RightOnly(_) | DiffType::Changed(_, _) => {
                files_to_grep.insert(entry.path);
            }
        }
    }

    Ok((files_to_grep, files_to_exclude))
}

/// Run local grep on a set of files.
fn run_local_grep(
    ctx: &ReqCtx<GrepOpts>,
    repo: &CoreRepo,
    pattern: &str,
    matcher: DynMatcher,
    relativizer: &RepoPathRelativizer,
    rev: &str,
) -> Result<u64> {
    let file_store = repo.file_store()?;
    let (_, manifest) = repo.resolve_manifest(&ctx.core, rev, matcher.clone())?;
    let (file_rx, first_error) = filewalk::walk_and_fetch(manifest, matcher, &file_store);

    let use_color = ctx.should_color();

    let match_count = if ctx.opts.files_with_matches {
        run_summary_grep_with_writer(
            ctx,
            relativizer,
            file_rx,
            &first_error,
            pattern,
            ctx.io().output(),
            use_color,
        )?
    } else {
        run_standard_grep_with_writer(
            ctx,
            relativizer,
            file_rx,
            &first_error,
            pattern,
            ctx.io().output(),
            use_color,
        )?
    };

    first_error.wait()?;

    Ok(match_count)
}

/// Run local grep on a set of files with JSON output.
fn run_local_grep_json(
    ctx: &ReqCtx<GrepOpts>,
    repo: &CoreRepo,
    pattern: &str,
    matcher: DynMatcher,
    relativizer: &RepoPathRelativizer,
    rev: &str,
    json_out: &mut JsonOutput,
) -> Result<u64> {
    use crate::grep_files_json;

    // Get the file store for reading file contents
    let file_store = repo.file_store()?;

    // Get the manifest for fetching files
    let (_, manifest) = repo.resolve_manifest(&ctx.core, rev, matcher.clone())?;

    // Walk and fetch the files
    let (file_rx, first_error) = filewalk::walk_and_fetch(manifest, matcher, &file_store);

    let match_count = grep_files_json(
        &ctx.opts,
        pattern,
        file_rx,
        &first_error,
        relativizer,
        json_out,
    )?;

    first_error.wait()?;

    Ok(match_count)
}

/// Parse the corpus revision from the biggrep revision line.
/// Formats:
///   - "#HASH:timestamp" (single shard)
///   - "#name1=HASH:timestamp,name2=HASH:timestamp,..." (multiple shards)
fn parse_corpus_revision(revision_line: &str) -> Option<String> {
    if revision_line.contains('=') {
        // Multiple shards format
        let mut revisions: Vec<&str> = revision_line
            .split(',')
            .filter_map(|shard| {
                let (_name, info) = shard.split_once('=')?;
                let rev = if info.contains(':') {
                    info.split_once(':')?.0
                } else {
                    info
                };
                Some(rev)
            })
            .collect();

        if revisions.is_empty() {
            return None;
        }

        // Sort for deterministic choice
        revisions.sort();
        Some(revisions[0].to_string())
    } else {
        // Single shard format
        let rev = if revision_line.contains(':') {
            revision_line.split_once(':')?.0
        } else {
            revision_line
        };
        Some(rev.to_string())
    }
}

/// Strip ANSI escape sequences from a string.
fn strip_ansi_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;

    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            result.push(c);
        }
    }

    result
}
