/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::LazyLock;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

/// Known client identity buckets for per-client memory tracking.
///
/// Bounded cardinality — adding a new service identity requires adding a variant.
/// This enables a completely lock-free registry using a fixed-size array of AtomicI64.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum ClientBucket {
    Quicksand = 0,
    Ondemand = 1,
    Sandcastle = 2,
    Other = 3,
}

impl ClientBucket {
    pub const COUNT: usize = 4;

    pub fn name(&self) -> &'static str {
        match self {
            Self::Quicksand => "quicksand",
            Self::Ondemand => "ondemand",
            Self::Sandcastle => "sandcastle",
            Self::Other => "other",
        }
    }

    pub const ALL: [ClientBucket; Self::COUNT] = [
        Self::Quicksand,
        Self::Ondemand,
        Self::Sandcastle,
        Self::Other,
    ];
}

impl From<Option<&str>> for ClientBucket {
    /// Convert a `main_client_id` string to a `ClientBucket`.
    ///
    /// The `main_client_id` is already extracted from request metadata (e.g., in the
    /// rate limiting middleware) and encodes the client identity as a string like
    /// `"SERVICE_IDENTITY:quicksand_builder"` or `"USER:johndoe"`.
    ///
    /// Priority: known service identities > Other.
    fn from(main_client_id: Option<&str>) -> Self {
        match main_client_id {
            Some(id) if id.contains("quicksand") => ClientBucket::Quicksand,
            Some(id) if id.contains("ondemand") => ClientBucket::Ondemand,
            Some(id) if id.contains("sandcastle") => ClientBucket::Sandcastle,
            _ => ClientBucket::Other,
        }
    }
}

/// Lock-free, process-global registry tracking estimated in-flight memory
/// per client identity bucket.
///
/// All operations are atomic — no locks, no contention on the critical path.
/// `top_consumer()` iterates exactly `ClientBucket::COUNT` elements.
///
/// Updated by `WeightTracker` during request processing, queried by the
/// rate limiting middleware to decide which client to shed.
pub struct ClientMemoryRegistry {
    buckets: [AtomicI64; ClientBucket::COUNT],
    total: AtomicI64,
}

impl ClientMemoryRegistry {
    pub fn new() -> Self {
        Self {
            buckets: std::array::from_fn(|_| AtomicI64::new(0)),
            total: AtomicI64::new(0),
        }
    }

    pub fn add_weight(&self, bucket: ClientBucket, weight: i64) {
        self.buckets[bucket as usize].fetch_add(weight, Ordering::Relaxed);
        self.total.fetch_add(weight, Ordering::Relaxed);
    }

    pub fn remove_weight(&self, bucket: ClientBucket, weight: i64) {
        self.buckets[bucket as usize].fetch_sub(weight, Ordering::Relaxed);
        self.total.fetch_sub(weight, Ordering::Relaxed);
    }

    pub fn get_client_memory(&self, bucket: ClientBucket) -> i64 {
        self.buckets[bucket as usize].load(Ordering::Relaxed)
    }

    pub fn total_memory(&self) -> i64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Find the client bucket consuming the most estimated memory.
    /// Returns `None` if all buckets are at zero.
    /// Iterates exactly `ClientBucket::COUNT` elements — O(1), no locks.
    pub fn top_consumer(&self) -> Option<(ClientBucket, i64)> {
        let mut max_bucket = None;
        let mut max_memory = 0i64;
        for &bucket in &ClientBucket::ALL {
            let mem = self.buckets[bucket as usize].load(Ordering::Relaxed);
            if mem > max_memory {
                max_memory = mem;
                max_bucket = Some(bucket);
            }
        }
        max_bucket.map(|b| (b, max_memory))
    }
}

/// Global singleton registry.
static GLOBAL_REGISTRY: LazyLock<ClientMemoryRegistry> = LazyLock::new(ClientMemoryRegistry::new);

pub fn global_client_memory_registry() -> &'static ClientMemoryRegistry {
    &GLOBAL_REGISTRY
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_client_memory_registry() {
        let registry = ClientMemoryRegistry::new();
        assert_eq!(registry.total_memory(), 0);
        assert_eq!(registry.top_consumer(), None);

        registry.add_weight(ClientBucket::Quicksand, 100);
        assert_eq!(registry.total_memory(), 100);
        assert_eq!(registry.get_client_memory(ClientBucket::Quicksand), 100);
        assert_eq!(
            registry.top_consumer(),
            Some((ClientBucket::Quicksand, 100))
        );

        registry.add_weight(ClientBucket::Ondemand, 200);
        assert_eq!(registry.total_memory(), 300);
        assert_eq!(registry.get_client_memory(ClientBucket::Ondemand), 200);
        assert_eq!(registry.top_consumer(), Some((ClientBucket::Ondemand, 200)));

        registry.add_weight(ClientBucket::Sandcastle, 300);
        assert_eq!(registry.total_memory(), 600);
        assert_eq!(registry.get_client_memory(ClientBucket::Sandcastle), 300);
        assert_eq!(
            registry.top_consumer(),
            Some((ClientBucket::Sandcastle, 300))
        );

        registry.add_weight(ClientBucket::Other, 400);
        assert_eq!(registry.total_memory(), 1000);
        assert_eq!(registry.get_client_memory(ClientBucket::Other), 400);
        assert_eq!(registry.top_consumer(), Some((ClientBucket::Other, 400)));

        registry.remove_weight(ClientBucket::Other, 400);
        assert_eq!(registry.total_memory(), 600);
        assert_eq!(registry.get_client_memory(ClientBucket::Other), 0);
        assert_eq!(
            registry.top_consumer(),
            Some((ClientBucket::Sandcastle, 300))
        );

        registry.remove_weight(ClientBucket::Sandcastle, 300);
        assert_eq!(registry.total_memory(), 300);
        assert_eq!(registry.get_client_memory(ClientBucket::Sandcastle), 0);
        assert_eq!(registry.top_consumer(), Some((ClientBucket::Ondemand, 200)));
    }
}
