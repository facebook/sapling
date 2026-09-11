/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Ordering;
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use blob::Blob;
use clap::Parser;

const MAX_CANDIDATE_SIZE: u64 = 2 * 1024 * 1024;
const SIZE_TOLERANCE_PERCENT: u64 = 5;

#[derive(Debug, Parser)]
#[command(
    about = "Find files near representative sizes and print keyed CAS BLAKE3 digests",
    after_help = "Candidates come from `sl files .` within `--scan-dir`. Only files whose size differs from the bucket target by less than 5% are eligible.\n\nOutput starts with the current commit hash, followed by TSV columns: bucket, target_bytes, actual_bytes, digest, and path."
)]
struct Options {
    /// Directory within the Sapling repository to scan
    #[arg(long, default_value = ".")]
    scan_dir: PathBuf,

    /// Number of candidates to select for each size bucket
    #[arg(long, default_value = "50")]
    count: NonZeroUsize,
}

#[derive(Clone, Debug)]
struct Candidate {
    fs_path: PathBuf,
    display_path: PathBuf,
    size: u64,
}

#[derive(Debug)]
struct Bucket {
    name: &'static str,
    target_size: u64,
    limit: usize,
    candidates: Vec<Candidate>,
}

#[derive(Default, Debug)]
struct ScanStats {
    paths: u64,
    regular_files: u64,
    too_large: u64,
    skipped: u64,
    stopped_early: bool,
}

impl Bucket {
    fn new(name: &'static str, target_size: u64, limit: usize) -> Self {
        Self {
            name,
            target_size,
            limit,
            candidates: Vec::new(),
        }
    }

    fn consider(&mut self, candidate: &Candidate) {
        if self.is_full() || !self.is_eligible(candidate) {
            return;
        }

        self.candidates.push(candidate.clone());
    }

    fn is_full(&self) -> bool {
        self.candidates.len() == self.limit
    }

    fn is_eligible(&self, candidate: &Candidate) -> bool {
        size_distance(candidate.size, self.target_size) * 100
            < self.target_size * SIZE_TOLERANCE_PERCENT
    }

    fn sort(&mut self) {
        let target_size = self.target_size;
        self.candidates
            .sort_by(|left, right| compare_candidate(target_size, left, right));
    }
}

fn compare_candidate(target_size: u64, left: &Candidate, right: &Candidate) -> Ordering {
    size_distance(left.size, target_size)
        .cmp(&size_distance(right.size, target_size))
        .then_with(|| left.size.cmp(&right.size))
        .then_with(|| left.display_path.cmp(&right.display_path))
}

fn size_distance(size: u64, target_size: u64) -> u64 {
    size.abs_diff(target_size)
}

fn main() -> Result<()> {
    let options = Options::parse();
    let count = options.count.get();
    let scan_dir = fs::canonicalize(&options.scan_dir)
        .with_context(|| format!("resolving scan directory {}", options.scan_dir.display()))?;
    let repo_root = repository_root(&scan_dir)?;
    let display_root = scan_dir.strip_prefix(&repo_root).with_context(|| {
        format!(
            "scan directory {} is outside repository root {}",
            scan_dir.display(),
            repo_root.display()
        )
    })?;
    let commit = current_commit(&repo_root)?;
    let mut buckets = vec![
        Bucket::new("small", 8 * 1024, count),
        Bucket::new("medium", 100 * 1024, count),
        Bucket::new("large", 1024 * 1024, count),
    ];

    let stats = scan_sl_files(&scan_dir, |candidate| {
        let candidate = Candidate {
            display_path: display_root.join(&candidate.display_path),
            ..candidate
        };
        for bucket in &mut buckets {
            bucket.consider(&candidate);
        }
        buckets.iter().all(Bucket::is_full)
    })?;

    for bucket in &mut buckets {
        bucket.sort();
    }

    eprintln!(
        "scanned paths={} regular_files={} skipped={} too_large={} stopped_early={}",
        stats.paths, stats.regular_files, stats.skipped, stats.too_large, stats.stopped_early
    );
    for bucket in &buckets {
        if bucket.candidates.len() < count {
            eprintln!(
                "warning: only found {} eligible candidates for {} target {} bytes",
                bucket.candidates.len(),
                bucket.name,
                bucket.target_size
            );
        }
    }

    println!("# commit {commit}");
    println!("bucket\ttarget_bytes\tactual_bytes\tdigest\tpath");
    for bucket in buckets {
        for candidate in bucket.candidates {
            let digest = digest_file(&candidate.fs_path)
                .with_context(|| format!("hashing {}", candidate.fs_path.display()))?;
            println!(
                "{}\t{}\t{}\t{}:{}\t{}",
                bucket.name,
                bucket.target_size,
                candidate.size,
                digest,
                candidate.size,
                candidate.display_path.display()
            );
        }
    }

    Ok(())
}

fn current_commit(root: &Path) -> Result<String> {
    sl_output(root, "whereami")
}

fn repository_root(root: &Path) -> Result<PathBuf> {
    sl_output(root, "root").map(PathBuf::from)
}

fn sl_output(root: &Path, subcommand: &str) -> Result<String> {
    let output = Command::new("sl")
        .arg(subcommand)
        .current_dir(root)
        .output()
        .with_context(|| format!("running `sl {subcommand}`"))?;
    if !output.status.success() {
        bail!(
            "`sl {subcommand}` exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let value = String::from_utf8(output.stdout)
        .with_context(|| format!("decoding `sl {subcommand}` output"))?;
    let value = value.trim();
    if value.is_empty() {
        bail!("`sl {subcommand}` returned empty output");
    }
    Ok(value.to_owned())
}

fn scan_sl_files(scan_dir: &Path, mut visit: impl FnMut(Candidate) -> bool) -> Result<ScanStats> {
    let mut child = Command::new("sl")
        .arg("files")
        .arg(".")
        .current_dir(scan_dir)
        .stdout(Stdio::piped())
        .spawn()
        .context("running `sl files`")?;
    let Some(stdout) = child.stdout.take() else {
        stop_sl_files(&mut child)?;
        return Err(anyhow!("failed to capture `sl files` stdout"));
    };
    let mut stats = ScanStats::default();

    let scan_result = (|| -> Result<()> {
        for line in BufReader::new(stdout).lines() {
            let path = line.context("reading `sl files` output")?;
            if consider_path(scan_dir, PathBuf::from(path), &mut stats, &mut visit) {
                stats.stopped_early = true;
                break;
            }
        }
        Ok(())
    })();
    if let Err(error) = scan_result {
        return match stop_sl_files(&mut child) {
            Ok(()) => Err(error),
            Err(cleanup_error) => {
                Err(error.context(format!("also failed to stop `sl files`: {cleanup_error:#}")))
            }
        };
    }

    if stats.stopped_early {
        stop_sl_files(&mut child)?;
        return Ok(stats);
    }

    let status = child.wait().context("waiting for `sl files`")?;
    if !status.success() {
        bail!("`sl files` exited with {status}");
    }

    Ok(stats)
}

fn stop_sl_files(child: &mut Child) -> Result<()> {
    if child
        .try_wait()
        .context("checking `sl files` process")?
        .is_none()
    {
        child.kill().context("stopping `sl files` process")?;
        child.wait().context("waiting for `sl files` process")?;
    }
    Ok(())
}

fn consider_path(
    scan_dir: &Path,
    display_path: PathBuf,
    stats: &mut ScanStats,
    visit: &mut impl FnMut(Candidate) -> bool,
) -> bool {
    stats.paths += 1;
    let fs_path = scan_dir.join(&display_path);
    consider_file(fs_path, display_path, stats, visit)
}

fn consider_file(
    fs_path: PathBuf,
    display_path: PathBuf,
    stats: &mut ScanStats,
    visit: &mut impl FnMut(Candidate) -> bool,
) -> bool {
    let Ok(metadata) = fs::symlink_metadata(&fs_path) else {
        stats.skipped += 1;
        return false;
    };
    if !metadata.file_type().is_file() {
        stats.skipped += 1;
        return false;
    }
    stats.regular_files += 1;
    if metadata.len() > MAX_CANDIDATE_SIZE {
        stats.too_large += 1;
        return false;
    }

    visit(Candidate {
        fs_path,
        display_path,
        size: metadata.len(),
    })
}

fn digest_file(path: &Path) -> Result<String> {
    let data = fs::read(path)?;
    Ok(Blob::from(data).blake3().to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(path: &str, size: u64) -> Candidate {
        Candidate {
            fs_path: PathBuf::from(path),
            display_path: PathBuf::from(path),
            size,
        }
    }

    #[test]
    fn bucket_accepts_only_eligible_candidates_until_full() {
        let mut bucket = Bucket::new("test", 100, 2);

        bucket.consider(&candidate("too-small", 0));
        bucket.consider(&candidate("lower-boundary", 95));
        bucket.consider(&candidate("upper-boundary", 105));
        assert!(bucket.candidates.is_empty());
        assert!(!bucket.is_full());

        bucket.consider(&candidate("lower", 96));
        bucket.consider(&candidate("upper", 104));
        assert!(bucket.is_full());

        bucket.consider(&candidate("ignored", 100));
        bucket.sort();

        let paths = bucket
            .candidates
            .iter()
            .map(|candidate| candidate.display_path.as_path())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec![Path::new("lower"), Path::new("upper")]);
    }
}
