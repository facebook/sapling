/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

#[cfg(not(target_os = "linux"))]
use memory_stats::memory_stats;

static MAX_MEMORY: AtomicUsize = AtomicUsize::new(0);

/// A structure that holds some basic statistics for Future.
#[derive(Clone, Debug)]
pub struct MemoryStats {
    /// Total RSS memory for the container in bytes.
    pub total_rss_bytes: usize,

    /// RSS memory free in bytes.
    pub rss_free_bytes: usize,

    /// RSS memory free in pct.
    pub rss_free_pct: f32,

    /// Linux VmSwap in bytes, excluding shared-memory swap; zero if unavailable.
    pub swap_bytes: usize,
}

pub fn set_max_memory(max_memory: usize) {
    MAX_MEMORY.store(max_memory, Ordering::Relaxed);
}

pub fn get_stats() -> Result<MemoryStats, String> {
    let max_memory = MAX_MEMORY.load(Ordering::Relaxed);
    if max_memory == 0 {
        return Err("max_memory is not set".to_string());
    }

    #[cfg(target_os = "linux")]
    let (rss_bytes, swap_bytes) = {
        use procfs::FromRead;
        use procfs::process::Status;

        let status = Status::from_file("/proc/self/status")
            .map_err(|err| format!("failed to read process memory: {err}"))?;
        memory_bytes_from_status(status.vmrss, status.vmswap).ok_or_else(|| {
            "missing VmRSS or invalid memory size in /proc/self/status".to_string()
        })?
    };
    #[cfg(not(target_os = "linux"))]
    let (rss_bytes, swap_bytes) = memory_stats()
        .map(|usage| (usage.physical_mem, 0))
        .ok_or_else(|| "failed to get memory stats".to_string())?;

    Ok(populate_stats(max_memory, rss_bytes, swap_bytes))
}

#[cfg(target_os = "linux")]
fn memory_bytes_from_status(rss_kib: Option<u64>, swap_kib: Option<u64>) -> Option<(usize, usize)> {
    let rss_bytes = usize::try_from(rss_kib?.checked_mul(1024)?).ok()?;
    let swap_bytes = usize::try_from(swap_kib.unwrap_or(0).checked_mul(1024)?).ok()?;
    Some((rss_bytes, swap_bytes))
}

fn populate_stats(max_memory: usize, used_mem: usize, swap_bytes: usize) -> MemoryStats {
    let free_mem = max_memory.saturating_sub(used_mem);
    MemoryStats {
        total_rss_bytes: max_memory,
        rss_free_bytes: free_mem,
        rss_free_pct: (free_mem as f64 / max_memory as f64) as f32 * 100.0,
        swap_bytes,
    }
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_get_stats() {
        let old = MAX_MEMORY.load(Ordering::Relaxed);
        let max_memory = 1024 * 1024 * 1024;
        MAX_MEMORY.store(max_memory, Ordering::Relaxed);

        let stats = get_stats();
        assert!(stats.is_ok());

        let stats = stats.unwrap();
        assert!(stats.rss_free_bytes < max_memory);
        assert!(stats.rss_free_pct > 0.0);
        assert!(stats.rss_free_pct < 100.0);

        MAX_MEMORY.store(old, Ordering::Relaxed);
    }

    #[cfg(target_os = "linux")]
    #[mononoke::test]
    fn missing_swap_preserves_rss_measurement() {
        assert_eq!(
            memory_bytes_from_status(Some(123), None),
            Some((123 * 1024, 0))
        );
        assert_eq!(
            memory_bytes_from_status(Some(123), Some(5)),
            Some((123 * 1024, 5 * 1024))
        );
        assert_eq!(memory_bytes_from_status(None, Some(5)), None);
        assert_eq!(memory_bytes_from_status(Some(u64::MAX), None), None);
        assert_eq!(memory_bytes_from_status(Some(123), Some(u64::MAX)), None);
    }

    #[mononoke::test]
    fn test_populate_stats() {
        let cases = [
            (
                32 * 1024 * 1024 * 1024, // max_memory
                29 * 1024 * 1024 * 1024, // used_mem
                32 * 1024 * 1024 * 1024, // expected_total_rss_bytes
                3 * 1024 * 1024 * 1024,  // expected_rss_free_bytes
                9.375,                   // expected_rss_free_pct
            ),
            (1000, 100, 1000, 900, 90.0),
            (1000, 1000, 1000, 0, 0.0),
            (1000, 1010, 1000, 0, 0.0),
        ];

        for (
            max_memory,
            used_mem,
            expected_total_rss_bytes,
            expected_rss_free_bytes,
            expected_rss_free_pct,
        ) in cases
        {
            let stats = populate_stats(max_memory, used_mem, 0);
            assert_eq!(
                stats.total_rss_bytes, expected_total_rss_bytes,
                "when max_memory={max_memory} and used_mem={used_mem}"
            );
            assert_eq!(
                stats.rss_free_bytes, expected_rss_free_bytes,
                "when max_memory={max_memory} and used_mem={used_mem}"
            );
            assert_eq!(
                stats.rss_free_pct, expected_rss_free_pct,
                "when max_memory={max_memory} and used_mem={used_mem}"
            );
        }
    }
}
