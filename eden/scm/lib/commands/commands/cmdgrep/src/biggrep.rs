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

use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use clidispatch::ReqCtx;
use clidispatch::abort;
use cmdutil::ConfigExt;
use cmdutil::Result;
use pathmatcher::DynMatcher;
use pathmatcher::Matcher;
use repo::CoreRepo;
use types::RepoPath;
use types::RepoPathBuf;
use types::path::RepoPathRelativizer;

use crate::GrepOpts;

/// Check if biggrep should be used and run it if so.
/// Returns Some(exit_code) if biggrep was used, None if we should fall back to local grep.
///
/// Parameters:
/// - `exact_files`: Exact file paths to scope biggrep search (from HintedMatcher)
/// - `matcher`: Used for filtering results (includes sparse profile intersection)
pub fn try_biggrep(
    ctx: &ReqCtx<GrepOpts>,
    repo: &CoreRepo,
    pattern: &str,
    exact_files: &[RepoPathBuf],
    matcher: &DynMatcher,
    relativizer: &RepoPathRelativizer,
    cwd: &Path,
    repo_root: Option<&Path>,
) -> Result<Option<u8>> {
    let config = repo.config();

    // Check if biggrep is explicitly enabled/disabled
    let use_biggrep = config.get_opt::<bool>("grep", "usebiggrep")?;

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
    } else {
        pattern.replace("-", r"\-")
    };

    // Choose client based on fixed_strings option
    let biggrep_client = if ctx.opts.fixed_strings {
        "bgs".to_string().into()
    } else {
        biggrep_client
    };

    // Build the biggrep command
    let mut cmd = Command::new(&biggrep_client);
    cmd.arg(&*biggrep_tier)
        .arg(&biggrep_corpus)
        .arg("re2")
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

    // Enable color if appropriate.
    if ctx.should_color() {
        cmd.arg("--color=on");
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

    // Execute biggrep with streaming output
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn()?;

    let stdout = child.stdout.take().expect("stdout should be captured");
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // First line contains the corpus revision info (starts with #)
    let first_line = match lines.next() {
        Some(Ok(line)) if line.starts_with('#') => line,
        Some(Ok(_line)) => {
            // First line doesn't start with #, unexpected format
            // Wait for child and fall back to local grep
            let _ = child.wait();
            return Ok(None);
        }
        Some(Err(e)) => {
            let _ = child.wait();
            return Err(e.into());
        }
        None => {
            // No output, fall back to local grep
            let _ = child.wait();
            return Ok(None);
        }
    };

    // Parse the corpus revision (we don't use it for now since we're skipping the status check)
    let _corpus_rev = parse_corpus_revision(&first_line[1..]);

    ctx.maybe_start_pager(config.as_ref())?;

    let mut match_count = 0;
    let files_with_matches = ctx.opts.files_with_matches;
    let include_line_number = ctx.opts.line_number;

    let mut out = ctx.io().output();

    // Process results as they stream in
    for line_result in lines {
        let line = line_result?;

        if line.is_empty() {
            continue;
        }

        // Parse biggrep output line: filename:lineno:colno:context
        // Or for files_with_matches mode: just filename
        // Or for binary files: "Binary file X matches"
        let parsed = if files_with_matches {
            Some((line.as_str(), None, None))
        } else if let Some((filename, rest)) = line.split_once(':') {
            if let Some((lineno, rest)) = rest.split_once(':') {
                if let Some((_colno, context)) = rest.split_once(':') {
                    Some((filename, Some(lineno), Some(context)))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let (filename, lineno, context) = match parsed {
            Some((f, l, c)) => (
                f.to_string(),
                l.map(|s| s.to_string()),
                c.map(|s| s.to_string()),
            ),
            None => {
                // Check for binary file match
                if line.starts_with("Binary file ") && line.ends_with(" matches") {
                    let filename = &line[12..line.len() - 8];
                    (filename.to_string(), None, None)
                } else {
                    // Unparsable line, just output as-is
                    writeln!(out, "{}", line)?;
                    continue;
                }
            }
        };

        // Strip ANSI escape sequences for matching
        let plain_filename = strip_ansi_escapes(&filename);

        // Filter to files matching the matcher (includes sparse profile)
        let repo_path = match RepoPath::from_str(&plain_filename) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if !matcher.matches_file(repo_path)? {
            continue;
        }

        match_count += 1;

        // Relativize the path
        let rel_path = relativizer.relativize(repo_path);

        // Replace the plain filename with the relativized one in the output
        // (preserving any ANSI escape sequences)
        let display_filename = filename.replace(&plain_filename, &rel_path);

        // Output the result
        if files_with_matches {
            writeln!(out, "{}", display_filename)?;
        } else if context.is_none() {
            // Binary file match
            writeln!(out, "Binary file {} matches", display_filename)?;
        } else if include_line_number {
            writeln!(
                out,
                "{}:{}:{}",
                display_filename,
                lineno.as_deref().unwrap_or(""),
                context.as_deref().unwrap_or("")
            )?;
        } else {
            writeln!(
                out,
                "{}:{}",
                display_filename,
                context.as_deref().unwrap_or("")
            )?;
        }
    }

    // Wait for the child process to complete
    let status = child.wait()?;

    // biggrep's exit status is 0 if a line is selected, 1 if no lines were selected
    if !matches!(status.code(), Some(0) | Some(1)) {
        abort!(
            "biggrep_client failed with exit code {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(Some(if match_count > 0 { 0 } else { 1 }))
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
