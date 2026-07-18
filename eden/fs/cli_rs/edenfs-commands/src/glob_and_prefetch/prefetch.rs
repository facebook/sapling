/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl prefetch

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_telemetry::collect_system_info;
use edenfs_telemetry::edenfs_events_mapper;
use edenfs_utils::path_from_bytes;
use num_format::Locale;
use num_format::ToFormattedString;
use thrift_types::edenfs::PrefetchStats;

use crate::ExitCode;
use crate::get_edenfs_instance;
use crate::glob_and_prefetch::common::CommonArgs;

#[derive(Parser, Debug)]
#[clap(
    about = "Prefetch content for matching file patterns. Glob patterns can be provided via a pattern file. This command does not do any filtering based on source control state or gitignore files."
)]
pub struct PrefetchCmd {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(
        long,
        help = "DEPRECATED: Do not print the names of the matching files"
    )]
    silent: bool,

    #[clap(long, help = "Do not prefetch files; only prefetch directories")]
    directories_only: bool,

    #[clap(long, help = "Run the prefetch in the background")]
    background: bool,

    #[clap(
        long,
        help = "Print the paths being prefetched. Does not work if using --background"
    )]
    debug_print: bool,

    #[clap(
        short = 'r',
        long,
        help = "Resolve patterns relative to the current working directory instead of the repo root"
    )]
    relative: bool,

    #[clap(
        long,
        help = "Print statistics about the prefetch operation (cache hits, timing, etc.)"
    )]
    stats: bool,
}

const KIB: f64 = 1024.0;
const MIB: f64 = KIB * 1024.0;
const GIB: f64 = MIB * 1024.0;

fn format_bytes(num_bytes: i64) -> String {
    let num_bytes = num_bytes as f64;
    if num_bytes < KIB {
        format!("{} B", num_bytes as i64)
    } else if num_bytes < MIB {
        format!("{:.1} KiB", num_bytes / KIB)
    } else if num_bytes < GIB {
        format!("{:.1} MiB", num_bytes / MIB)
    } else {
        format!("{:.2} GiB", num_bytes / GIB)
    }
}

/// Format large numbers with commas for readability.
fn format_count(count: i64) -> String {
    count.to_formatted_string(&Locale::en)
}

/// Format a float with 1 decimal place and comma-grouped integer part, e.g.
/// 1234567.8 -> "1,234,567.8". Rounding is delegated to Rust's built-in
/// `{:.1}` float formatting (correctly rounds the true underlying binary
/// value) rather than reimplemented by hand; only the integer part of the
/// resulting string is then comma-grouped.
fn format_float_with_commas(value: f64) -> String {
    let formatted = format!("{value:.1}");
    let (int_part, decimal_part) = formatted
        .split_once('.')
        .expect("a value formatted with 1 decimal place always contains '.'");
    let int_value: i64 = int_part
        .parse()
        .expect("the integer part of a {:.1}-formatted f64 always parses as i64");
    format!("{}.{}", format_count(int_value), decimal_part)
}

/// Print prefetch statistics in a human-readable format.
/// actual_total_ms is the actual wall clock time from start to finish (when preload is used).
fn print_prefetch_stats(
    stats: &PrefetchStats,
    globs: &[String],
    preload_duration_ms: Option<i64>,
    actual_total_ms: Option<i64>,
) {
    println!();
    println!("=== Prefetch Statistics ===");
    println!();

    // Summary section
    println!("Summary:");
    if !globs.is_empty() {
        println!("  Globs:            {}", globs.join(" "));
    }
    println!(
        "  Files prefetched: {}",
        format_count(stats.filesPrefetched)
    );
    if stats.filesFailed > 0 {
        println!("  Files failed:     {}", format_count(stats.filesFailed));
    }

    // Timing section
    let prefetch_secs = stats.totalDurationMs as f64 / 1000.0;
    println!("  Prefetch time:    {prefetch_secs:.2} s");

    // total_secs is the user-observed wall-clock time for the whole
    // operation; when preload runs it dominates by 5x or more, so we
    // use it (not prefetch_secs alone) as the denominator for file
    // throughput below.
    let total_secs = if let Some(preload_ms) = preload_duration_ms {
        let preload_secs = preload_ms as f64 / 1000.0;
        println!("  Preload time:     {preload_secs:.2} s");
        // Use actual wall clock time if available (reflects interleaved
        // execution), otherwise fall back to sum of prefetch + preload.
        let total = if let Some(actual_ms) = actual_total_ms {
            actual_ms as f64 / 1000.0
        } else {
            prefetch_secs + preload_secs
        };
        println!("  Total time:       {total:.2} s");
        total
    } else {
        prefetch_secs
    };

    if total_secs > 0.0 {
        // Files-per-second over the FULL wall-clock duration — what the
        // user actually observes. Anchoring this on prefetch_secs alone
        // makes preload-heavy runs look 5-9x faster than reality.
        let throughput = stats.filesPrefetched as f64 / total_secs;
        println!(
            "  Throughput:       {} files/s",
            format_float_with_commas(throughput)
        );
        // Network throughput: decompressed bytes pulled over the wire,
        // divided by the prefetch phase (which is when network fetches
        // happen). Distinct from Throughput above — this measures the
        // daemon's effective network bandwidth, not end-user rate.
        let network_bytes = stats.blobBytesFromNetwork + stats.treeBytesFromNetwork;
        if network_bytes > 0 && prefetch_secs > 0.0 {
            let net_mib_s = (network_bytes as f64 / MIB) / prefetch_secs;
            println!(
                "  Network:          {} MiB/s (during prefetch)",
                format_float_with_commas(net_mib_s)
            );
        }
    }

    // Calculate and display tree cache hit rate with counts and bytes
    let total_trees =
        stats.treesFromMemoryCache + stats.treesFromDiskCache + stats.treesFromNetwork;
    let total_tree_bytes =
        stats.treeBytesFromMemoryCache + stats.treeBytesFromDiskCache + stats.treeBytesFromNetwork;
    if total_trees > 0 {
        let tree_cached = stats.treesFromMemoryCache + stats.treesFromDiskCache;
        let tree_bytes_cached = stats.treeBytesFromMemoryCache + stats.treeBytesFromDiskCache;
        let tree_hit_rate = 100.0 * tree_cached as f64 / total_trees as f64;
        println!(
            "  Tree cache:       {:.2}% ({} of {}, {} of {})",
            tree_hit_rate,
            format_count(tree_cached),
            format_count(total_trees),
            format_bytes(tree_bytes_cached),
            format_bytes(total_tree_bytes)
        );
    }

    // Calculate and display blob cache hit rate with counts and bytes
    let total_blobs =
        stats.blobsFromMemoryCache + stats.blobsFromDiskCache + stats.blobsFromNetwork;
    let total_blob_bytes =
        stats.blobBytesFromMemoryCache + stats.blobBytesFromDiskCache + stats.blobBytesFromNetwork;
    if total_blobs > 0 {
        let blob_cached = stats.blobsFromMemoryCache + stats.blobsFromDiskCache;
        let blob_bytes_cached = stats.blobBytesFromMemoryCache + stats.blobBytesFromDiskCache;
        let blob_hit_rate = 100.0 * blob_cached as f64 / total_blobs as f64;
        println!(
            "  Blob cache:       {:.2}% ({} of {}, {} of {})",
            blob_hit_rate,
            format_count(blob_cached),
            format_count(total_blobs),
            format_bytes(blob_bytes_cached),
            format_bytes(total_blob_bytes)
        );
    }

    println!("  Overall cache:    {:.1}%", stats.cacheHitRate);
    println!();

    // Trees detail section
    println!(
        "Trees: {} (data volume: {})",
        format_count(total_trees),
        format_bytes(total_tree_bytes)
    );
    println!(
        "  Memory cache: {}",
        format_count(stats.treesFromMemoryCache)
    );
    println!("  Disk cache:   {}", format_count(stats.treesFromDiskCache));
    println!("  Network:      {}", format_count(stats.treesFromNetwork));
    println!();

    // Blobs detail section (reuse total_blobs and total_blob_bytes from above)
    println!(
        "Blobs: {} (data volume: {})",
        format_count(total_blobs),
        format_bytes(total_blob_bytes)
    );
    println!(
        "  Memory cache: {}",
        format_count(stats.blobsFromMemoryCache)
    );
    println!("  Disk cache:   {}", format_count(stats.blobsFromDiskCache));
    println!("  Network:      {}", format_count(stats.blobsFromNetwork));
}

impl PrefetchCmd {
    fn new_sample(&self, mount_point: &Path) -> edenfs_telemetry::EdenSample {
        let mut sample = edenfs_telemetry::EdenSample::new();
        collect_system_info(&mut sample, edenfs_events_mapper);
        sample.add_string("logged_by", "cli_rs");
        sample.add_string("type", "prefetch");
        sample.add_string(
            "checkout",
            mount_point
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default(),
        );
        sample.add_bool("directories_only", self.directories_only);
        sample.add_bool("background", self.background);
        if let Some(pattern_file) = self.common.pattern_file.as_ref() {
            sample.add_string("pattern_file", pattern_file.to_str().unwrap_or_default());
        }
        if !self.common.pattern.is_empty() {
            sample.add_string_list("patterns", self.common.pattern.clone());
        }
        sample
    }
}

#[async_trait]
impl crate::Subcommand for PrefetchCmd {
    async fn run(&self) -> Result<ExitCode> {
        #[cfg(fbcode_build)]
        crate::init_enable_xplatlogger_events().await;
        let instance = get_edenfs_instance();
        let client = instance.get_client();
        let (mount_point, search_root) = self.common.get_mount_point_and_search_root()?;

        let mut sample = self.new_sample(&mount_point);

        let patterns = self.common.load_patterns()?;
        let silent = self.silent || !self.debug_print;
        let return_prefetched_files = !(self.background || silent);

        let optional_search_root: Option<&PathBuf> = if self.relative {
            Some(&search_root)
        } else {
            None
        };

        let result = match client
            .prefetch_files(
                &mount_point,
                patterns.clone(),
                self.directories_only,
                None,
                optional_search_root,
                Some(self.background),
                None,
                return_prefetched_files,
                self.stats,
            )
            .await
        {
            Ok(r) => {
                sample.add_bool("success", true);
                r
            }
            Err(e) => {
                sample.add_bool("success", false);
                sample.add_string("error", format!("{e:#}").as_str());
                return Err(e.into());
            }
        };

        // NOTE: Is the really still needed? We should not be falling back at all anymore.
        sample.add_bool("prefetchV2_fallback", false);

        if return_prefetched_files {
            if !patterns.is_empty()
                && result
                    .prefetched_files
                    .as_ref()
                    .is_none_or(|pf| pf.matching_files.is_empty())
            {
                eprint!("No files were matched by the pattern");
                if !patterns.is_empty() {
                    eprint!("s");
                }
                eprintln!(" specified.\nSee `eden prefetch -h` for docs on pattern matching.");
            }

            if let Some(prefetched_files) = &result.prefetched_files {
                sample.add_int(
                    "files_fetched",
                    prefetched_files.matching_files.len() as i64,
                );

                if self.debug_print {
                    for file in &prefetched_files.matching_files {
                        println!("{}", path_from_bytes(file)?.display());
                    }
                }
            }
        }

        // Print stats if requested
        if self.stats {
            if let Some(stats) = &result.stats {
                print_prefetch_stats(stats, &patterns, None, None);
            } else {
                eprintln!("Prefetch stats unavailable");
            }
        }

        #[cfg(fbcode_build)]
        crate::send_edenfs_event(sample);
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(1048576), "1.0 MiB");
        assert_eq!(format_bytes(1572864), "1.5 MiB");
        assert_eq!(format_bytes(1073741824), "1.00 GiB");
        assert_eq!(format_bytes(1610612736), "1.50 GiB");
    }

    #[test]
    fn test_format_count() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(1), "1");
        assert_eq!(format_count(12), "12");
        assert_eq!(format_count(123), "123");
        assert_eq!(format_count(1234), "1,234");
        assert_eq!(format_count(12345), "12,345");
        assert_eq!(format_count(123456), "123,456");
        assert_eq!(format_count(1234567), "1,234,567");
        assert_eq!(format_count(1000000000), "1,000,000,000");
    }

    #[test]
    fn test_format_float_with_commas() {
        assert_eq!(format_float_with_commas(0.0), "0.0");
        assert_eq!(format_float_with_commas(1.0), "1.0");
        assert_eq!(format_float_with_commas(1.5), "1.5");
        assert_eq!(format_float_with_commas(1234.5), "1,234.5");
        assert_eq!(format_float_with_commas(1234567.8), "1,234,567.8");
    }

    #[test]
    fn test_format_float_with_commas_rounding() {
        assert_eq!(format_float_with_commas(5.95), "6.0");
        assert_eq!(format_float_with_commas(5.94), "5.9");
        assert_eq!(format_float_with_commas(9.99), "10.0");
        assert_eq!(format_float_with_commas(999.95), "1,000.0");
        assert_eq!(format_float_with_commas(0.04), "0.0");
        assert_eq!(format_float_with_commas(0.05), "0.1");
    }

    #[test]
    fn test_format_count_negative() {
        assert_eq!(format_count(-1), "-1");
        assert_eq!(format_count(-1234), "-1,234");
    }
}
