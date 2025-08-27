/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Local caching implementation for snapshot cache items using indexedlog.
//!
//! The cache serves dual purposes: it stores both file content blobs (indexed by their
//! content-addressable ContentId) and snapshot changeset metadata (indexed by their
//! BonsaiChangesetId). This unified caching approach optimizes both file retrieval
//! and changeset metadata access during snapshot operations.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use edenapi_types::BonsaiChangesetId;
use edenapi_types::CacheableSnapshot;
use edenapi_types::ContentId;
use indexedlog::log::IndexOutput;
use minibytes::Bytes;
use revisionstore::indexedlogutil::Store;
use revisionstore::indexedlogutil::StoreOpenOptions;
use tracing::debug;
use tracing::warn;

/// Default maximum size of cache items to cache (10MB)
const DEFAULT_MAX_CACHE_FILE_SIZE: usize = 10 * 1024 * 1024;

/// Default auto-sync threshold for indexedlog (1MB)
const DEFAULT_AUTO_SYNC_THRESHOLD: u64 = 1024 * 1024;

/// Default maximum number of log files to keep (for rotation)
const DEFAULT_MAX_LOG_COUNT: u8 = 3;

/// Default maximum bytes per log file (for rotation) - 3GB
const DEFAULT_MAX_BYTES_PER_LOG: u64 = 3 * 1024 * 1024 * 1024;

/// Cache entry format version
const CACHE_ENTRY_VERSION: u8 = 1;

/// Size of the entry header (version + ContentId + content size)
const fn entry_header_size() -> usize {
    1 + ContentId::len() + 8
}

/// Configuration for the snapshot file cache
#[derive(Debug, Clone)]
pub struct SnapshotCacheConfig {
    /// Path to the cache directory (if None, caching is disabled)
    pub cache_path: Option<std::path::PathBuf>,
    /// Maximum size of individual files to cache
    pub max_file_size: usize,
    /// Auto-sync threshold for indexedlog
    pub auto_sync_threshold: Option<u64>,
    /// Maximum number of log files to keep (for rotation)
    pub max_log_count: u8,
    /// Maximum bytes per log file (for rotation)
    pub max_bytes_per_log: u64,
}

impl Default for SnapshotCacheConfig {
    fn default() -> Self {
        Self {
            cache_path: None,
            max_file_size: DEFAULT_MAX_CACHE_FILE_SIZE,
            auto_sync_threshold: Some(DEFAULT_AUTO_SYNC_THRESHOLD),
            max_log_count: DEFAULT_MAX_LOG_COUNT,
            max_bytes_per_log: DEFAULT_MAX_BYTES_PER_LOG,
        }
    }
}

impl SnapshotCacheConfig {
    /// Create configuration from a Mercurial config
    pub fn from_config(config: &dyn Config) -> Result<Self> {
        let cache_path = config
            .get_opt::<String>("snapshot", "cache-path")?
            .and_then(|path_str| {
                if path_str.trim().is_empty() {
                    debug!("Snapshot cache disabled - cache path is empty");
                    None
                } else {
                    let repo_name = config
                        .get("remotefilelog", "reponame")
                        .map_or("default".to_string(), |s| s.to_string());

                    let path = if path_str.trim() == "hgcache" {
                        // Special handling for "hgcache" - use remotefilelog.cachepath
                        // and append repo name and "snapshots"
                        match config.get_opt::<String>("remotefilelog", "cachepath") {
                            Ok(Some(cache_path)) => {
                                let mut hg_cache_path = std::path::PathBuf::from(cache_path);
                                hg_cache_path.push(&repo_name);
                                hg_cache_path.push("snapshots");
                                hg_cache_path
                            }
                            _ => {
                                debug!("Snapshot cache disabled - remotefilelog.cachepath not set for hgcache mode");
                                return None;
                            }
                        }
                    } else {
                        let mut path = std::path::PathBuf::from(path_str);
                        path.push(&repo_name);
                        path
                    };

                    Some(path)
                }
            });

        let max_file_size = config
            .get_opt::<ByteCount>("snapshot", "cache.max-file-size")?
            .map_or(DEFAULT_MAX_CACHE_FILE_SIZE, |bc| bc.value() as usize);

        let auto_sync_threshold = config
            .get_opt::<ByteCount>("snapshot", "cache.auto-sync-threshold")?
            .map(|bc| bc.value());

        let max_log_count = config
            .get_opt::<u8>("snapshot", "cache.max-log-count")?
            .unwrap_or(DEFAULT_MAX_LOG_COUNT);

        let max_bytes_per_log = config
            .get_opt::<ByteCount>("snapshot", "cache.max-bytes-per-log")?
            .map_or(DEFAULT_MAX_BYTES_PER_LOG, |bc| bc.value());

        Ok(Self {
            cache_path,
            max_file_size,
            auto_sync_threshold,
            max_log_count,
            max_bytes_per_log,
        })
    }

    /// Set cache path (None disables caching)
    pub fn cache_path<P: Into<std::path::PathBuf>>(mut self, path: Option<P>) -> Self {
        self.cache_path = path.map(|p| p.into());
        self
    }

    /// Set maximum file size to cache
    pub fn max_file_size(mut self, size: usize) -> Self {
        self.max_file_size = size;
        self
    }

    /// Set auto-sync threshold
    pub fn auto_sync_threshold(mut self, threshold: Option<u64>) -> Self {
        self.auto_sync_threshold = threshold;
        self
    }

    /// Set maximum log count for rotation
    pub fn max_log_count(mut self, count: u8) -> Self {
        self.max_log_count = count;
        self
    }

    /// Set maximum bytes per log file
    pub fn max_bytes_per_log(mut self, bytes: u64) -> Self {
        self.max_bytes_per_log = bytes;
        self
    }

    /// Check if caching is enabled
    pub fn is_enabled(&self) -> bool {
        self.cache_path.is_some()
    }
}

/// Local cache for snapshot file content using indexedlog
pub struct SnapshotFileCache {
    store: Store,
    config: SnapshotCacheConfig,
}

impl SnapshotFileCache {
    /// Create a new snapshot file cache at the given directory with default config
    pub fn new<P: AsRef<Path>>(cache_dir: P) -> Result<Self> {
        Self::with_config(cache_dir, SnapshotCacheConfig::default())
    }

    /// Create a new snapshot file cache with custom configuration
    pub fn with_config<P: AsRef<Path>>(cache_dir: P, config: SnapshotCacheConfig) -> Result<Self> {
        let cache_dir = cache_dir.as_ref();

        let mut store_options = StoreOpenOptions::new(
            &std::collections::BTreeMap::<&str, &str>::new(),
        )
        .index("content_id", |data: &[u8]| {
            // Entry format: [version(1 byte)][content_id(32 bytes)][content_size(8 bytes)][content]
            if data.len() < 1 + ContentId::len() {
                return vec![];
            }
            vec![IndexOutput::Reference(1..(1 + ContentId::len()) as u64)]
        });

        if let Some(threshold) = config.auto_sync_threshold {
            store_options = store_options.auto_sync_threshold(threshold);
        }

        store_options = store_options.max_log_count(config.max_log_count);
        store_options = store_options.max_bytes_per_log(config.max_bytes_per_log);

        let store = store_options
            .rotated(cache_dir)
            .with_context(|| format!("Failed to open snapshot cache at {:?}", cache_dir))?;

        Ok(Self { store, config })
    }

    /// Create from Mercurial config
    pub fn from_config<P: AsRef<Path>>(cache_dir: P, config: &dyn Config) -> Result<Self> {
        let cache_config = SnapshotCacheConfig::from_config(config)?;
        Self::with_config(cache_dir, cache_config)
    }

    /// Store cache item, calculating the ContentId automatically
    pub fn store(&mut self, content: &Bytes) -> Result<ContentId> {
        let content_id = crate::util::calc_contentid(content);
        self.store_with_content_id(content_id, content)?;
        Ok(content_id)
    }

    /// Store cache item using the provided ContentId
    /// This avoids recalculating the hash if we already have it from upload
    pub fn store_with_content_id(&mut self, content_id: ContentId, content: &Bytes) -> Result<()> {
        // Skip caching very large items to avoid bloating the cache
        if content.len() > self.config.max_file_size {
            debug!(
                "Skipping cache for large item ({} bytes > {} bytes)",
                content.len(),
                self.config.max_file_size
            );
            return Ok(());
        }

        // Check if content is already cached
        if self.contains(&content_id)? {
            debug!("Content already cached for ContentId: {}", content_id);
            return Ok(());
        }

        let mut entry = Vec::with_capacity(entry_header_size() + content.len());
        entry.push(CACHE_ENTRY_VERSION);
        entry.extend_from_slice(content_id.as_ref());
        entry.extend_from_slice(&(content.len() as u64).to_le_bytes());
        entry.extend_from_slice(content);

        self.store.append(&entry).with_context(|| {
            format!(
                "Failed to store content in cache (ContentId: {})",
                content_id
            )
        })?;

        debug!(
            "Cached content ({} bytes) with ContentId: {}",
            content.len(),
            content_id
        );

        Ok(())
    }

    /// Check if content exists in the cache by ContentId (without reading the content)
    pub fn contains(&self, content_id: &ContentId) -> Result<bool> {
        let store_read = self.store.read();
        store_read
            .contains(0, content_id.as_ref())
            .with_context(|| {
                format!(
                    "Failed to check if ContentId {} exists in cache",
                    content_id
                )
            })
    }

    /// Retrieve cache item from the cache by ContentId
    pub fn get(&self, content_id: &ContentId) -> Result<Option<Bytes>> {
        let store_read = self.store.read();
        let lookup_result = store_read
            .lookup(0, content_id.as_ref())
            .with_context(|| format!("Failed to lookup ContentId {} in cache", content_id))?;

        for entry_result in lookup_result {
            let entry = entry_result.with_context(|| {
                format!("Failed to read cache entry for ContentId {}", content_id)
            })?;

            if entry.len() < entry_header_size() {
                warn!(
                    "Invalid cache entry format (too short): {} bytes",
                    entry.len()
                );
                continue;
            }

            // Check version byte
            let version = entry[0];
            if version != CACHE_ENTRY_VERSION {
                warn!(
                    "Unsupported cache entry version: {} (expected {})",
                    version, CACHE_ENTRY_VERSION
                );
                continue;
            }

            let stored_content_id = &entry[1..1 + ContentId::len()];
            if stored_content_id != content_id.as_ref() {
                warn!("ContentId mismatch in cache entry");
                continue;
            }

            let content_size_bytes = &entry[1 + ContentId::len()..entry_header_size()];
            let content_size = u64::from_le_bytes(
                content_size_bytes
                    .try_into()
                    .context("Failed to parse content size")?,
            ) as usize;

            let content_start = entry_header_size();
            if entry.len() < content_start + content_size {
                warn!("Invalid cache entry format (content too short)");
                continue;
            }

            let content = &entry[content_start..content_start + content_size];

            debug!(
                "Cache hit for ContentId {} ({} bytes)",
                content_id,
                content.len()
            );

            return Ok(Some(store_read.slice_to_bytes(content)));
        }

        Ok(None)
    }

    /// Flush any pending writes to disk
    pub fn flush(&mut self) -> Result<()> {
        self.store
            .flush()
            .context("Failed to flush snapshot cache to disk")?;
        Ok(())
    }
}

/// Optional cache wrapper that handles disabled caching gracefully
pub struct OptionalSnapshotFileCache {
    inner: Option<SnapshotFileCache>,
}

impl OptionalSnapshotFileCache {
    /// Create from configuration - returns None if caching is disabled
    pub fn from_config(config: &dyn Config) -> Result<Self> {
        let cache_config = SnapshotCacheConfig::from_config(config)?;
        let inner = if let Some(cache_path) = cache_config.cache_path.clone() {
            debug!("Enabling snapshot cache at path: {:?}", cache_path);
            Some(SnapshotFileCache::with_config(&cache_path, cache_config)?)
        } else {
            debug!("Snapshot cache disabled - no cache path configured");
            None
        };

        Ok(Self { inner })
    }

    /// Create with explicit configuration
    pub fn with_config(config: SnapshotCacheConfig) -> Result<Self> {
        let inner = if let Some(cache_path) = config.cache_path.clone() {
            debug!("Enabling snapshot cache at path: {:?}", cache_path);
            Some(SnapshotFileCache::with_config(&cache_path, config)?)
        } else {
            debug!("Snapshot cache disabled - no cache path configured");
            None
        };

        Ok(Self { inner })
    }

    /// Store cache item, calculating the ContentId automatically (no-op if caching is disabled)
    /// This assumes that the intention is to store content-addressable blobs, not metadata
    pub fn store(&mut self, content: &Bytes) -> Result<Option<ContentId>> {
        if let Some(cache) = &mut self.inner {
            Ok(Some(cache.store(content)?))
        } else {
            Ok(None)
        }
    }

    /// Store cache item (no-op if caching is disabled)
    pub fn store_with_content_id(&mut self, content_id: ContentId, content: &Bytes) -> Result<()> {
        if let Some(cache) = &mut self.inner {
            cache.store_with_content_id(content_id, content)
        } else {
            Ok(())
        }
    }

    /// Check if content exists in the cache (returns false if caching is disabled)
    pub fn contains(&self, content_id: &ContentId) -> Result<bool> {
        if let Some(cache) = &self.inner {
            cache.contains(content_id)
        } else {
            Ok(false)
        }
    }

    /// Retrieve cache item from the cache (returns None if caching is disabled)
    pub fn get(&self, content_id: &ContentId) -> Result<Option<Bytes>> {
        if let Some(cache) = &self.inner {
            cache.get(content_id)
        } else {
            Ok(None)
        }
    }

    /// Flush any pending writes to disk (no-op if caching is disabled)
    pub fn flush(&mut self) -> Result<()> {
        if let Some(cache) = &mut self.inner {
            cache.flush()
        } else {
            Ok(())
        }
    }

    /// Check if caching is enabled
    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }
}

/// Thread-safe wrapper around OptionalSnapshotFileCache
pub struct SharedSnapshotFileCache {
    inner: Arc<parking_lot::Mutex<OptionalSnapshotFileCache>>,
}

impl SharedSnapshotFileCache {
    /// Create from configuration (thread-safe)
    pub fn from_config(config: &dyn Config) -> Result<Self> {
        let cache = OptionalSnapshotFileCache::from_config(config)?;
        Ok(Self {
            inner: Arc::new(parking_lot::Mutex::new(cache)),
        })
    }

    /// Create with explicit configuration (thread-safe)
    pub fn with_config(config: SnapshotCacheConfig) -> Result<Self> {
        let cache = OptionalSnapshotFileCache::with_config(config)?;
        Ok(Self {
            inner: Arc::new(parking_lot::Mutex::new(cache)),
        })
    }

    /// Store cache item, calculating the ContentId automatically (thread-safe)
    pub fn store(&self, content: &Bytes) -> Result<Option<ContentId>> {
        self.inner.lock().store(content)
    }

    /// Store cache item using ContentId (thread-safe)
    pub fn store_with_content_id(&self, content_id: ContentId, content: &Bytes) -> Result<()> {
        self.inner.lock().store_with_content_id(content_id, content)
    }

    /// Check if content exists in the cache (thread-safe)
    pub fn contains(&self, content_id: &ContentId) -> Result<bool> {
        self.inner.lock().contains(content_id)
    }

    /// Retrieve cache item from the cache by ContentId (thread-safe)
    pub fn get(&self, content_id: &ContentId) -> Result<Option<Bytes>> {
        self.inner.lock().get(content_id)
    }

    /// Flush any pending writes to disk (thread-safe)
    pub fn flush(&self) -> Result<()> {
        self.inner.lock().flush()
    }

    /// Check if caching is enabled (thread-safe)
    pub fn is_enabled(&self) -> bool {
        self.inner.lock().is_enabled()
    }

    /// Store CacheableSnapshot in the cache using BonsaiChangesetId as key
    /// Since BonsaiChangesetId and ContentId are both 32 bytes,
    /// we can reuse the same cache infrastructure to cache the bonsai changeset and its metadata
    pub fn store_snapshot(
        &self,
        changeset_id: BonsaiChangesetId,
        snapshot: &CacheableSnapshot,
    ) -> Result<()> {
        // Serialize the snapshot using CBOR
        let serialized = serde_cbor::to_vec(snapshot)
            .context("Failed to serialize CacheableSnapshot to CBOR")?;
        let content = Bytes::from(serialized);

        // Convert BonsaiChangesetId to ContentId for storage
        // Both are 32-byte hashes, so we can safely reinterpret BonsaiChangesetId as ContentId
        // with compile time checks
        let storage_key = ContentId::from_byte_array(changeset_id.into_byte_array());
        self.store_with_content_id(storage_key, &content)
    }

    /// Retrieve CacheableSnapshot from the cache using BonsaiChangesetId as key
    pub fn get_snapshot(
        &self,
        changeset_id: &BonsaiChangesetId,
    ) -> Result<Option<CacheableSnapshot>> {
        // Convert BonsaiChangesetId to ContentId for lookup
        let storage_key = ContentId::from_slice(changeset_id.as_ref())
            .context("Failed to create ContentId from BonsaiChangesetId bytes")?;

        let content = self.get(&storage_key)?;
        if let Some(content) = content {
            // Deserialize from CBOR
            let snapshot = serde_cbor::from_slice(&content)
                .context("Failed to deserialize CacheableSnapshot from CBOR")?;
            Ok(Some(snapshot))
        } else {
            Ok(None)
        }
    }

    /// Check if snapshot exists in the cache by BonsaiChangesetId
    pub fn contains_snapshot(&self, changeset_id: &BonsaiChangesetId) -> Result<bool> {
        let storage_key = ContentId::from_slice(changeset_id.as_ref())
            .context("Failed to create ContentId from BonsaiChangesetId bytes")?;
        self.contains(&storage_key)
    }
}

impl Clone for SharedSnapshotFileCache {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for SnapshotFileCache {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            warn!("Failed to flush snapshot cache on drop: {}", e);
        }
    }
}

impl Drop for OptionalSnapshotFileCache {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            warn!("Failed to flush optional snapshot cache on drop: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use types::Parents;

    use super::*;
    use crate::util::calc_contentid;

    #[test]
    fn test_cache_store_and_retrieve() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut cache = SnapshotFileCache::new(temp_dir.path())?;

        let content = Bytes::from("Hello, world!");
        let content_id = calc_contentid(&content);

        cache.store_with_content_id(content_id, &content)?;

        let retrieved = cache.get(&content_id)?;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), content);

        Ok(())
    }

    #[test]
    fn test_cache_store_auto_calc_content_id() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let mut cache = SnapshotFileCache::new(temp_dir.path())?;

        let content = Bytes::from("Auto-calculated ContentId test");
        let returned_content_id = cache.store(&content)?;

        // Verify the returned ContentId matches what we would calculate manually
        let expected_content_id = calc_contentid(&content);
        assert_eq!(returned_content_id, expected_content_id);

        // Verify we can retrieve the content using the returned ContentId
        let retrieved = cache.get(&returned_content_id)?;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), content);

        Ok(())
    }

    #[test]
    fn test_cache_miss() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let cache = SnapshotFileCache::new(temp_dir.path())?;

        let fake_content_id = ContentId::from_byte_array([0u8; 32]);
        let result = cache.get(&fake_content_id)?;
        assert!(result.is_none());

        Ok(())
    }

    #[test]
    fn test_large_file_skipping() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config = SnapshotCacheConfig::default().max_file_size(100); // Small limit for testing
        let mut cache = SnapshotFileCache::with_config(temp_dir.path(), config)?;

        // Create content larger than the configured max file size
        let large_content = Bytes::from(vec![0u8; 101]);
        let content_id = calc_contentid(&large_content);

        cache.store_with_content_id(content_id, &large_content)?;

        // Should not cache the content
        let retrieved = cache.get(&content_id)?;
        assert!(retrieved.is_none());

        Ok(())
    }

    #[test]
    fn test_shared_cache() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config = SnapshotCacheConfig::default().cache_path(Some(temp_dir.path()));
        let cache = SharedSnapshotFileCache::with_config(config)?;

        let content = Bytes::from("Shared cache test");
        let content_id = calc_contentid(&content);

        cache.store_with_content_id(content_id, &content)?;

        let retrieved = cache.get(&content_id)?;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), content);

        Ok(())
    }

    #[test]
    fn test_disabled_cache() -> Result<()> {
        let config = SnapshotCacheConfig::default(); // No cache path = disabled
        let mut cache = OptionalSnapshotFileCache::with_config(config)?;

        let content = Bytes::from("This won't be cached");
        let content_id = calc_contentid(&content);

        // Should be no-op
        cache.store_with_content_id(content_id, &content)?;

        // Should return None
        let retrieved = cache.get(&content_id)?;
        assert!(retrieved.is_none());

        // Test auto-calc store method - should return None for disabled cache
        let auto_result = cache.store(&content)?;
        assert!(auto_result.is_none());

        // Should report as disabled
        assert!(!cache.is_enabled());

        Ok(())
    }

    #[test]
    fn test_shared_cache_auto_calc() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config = SnapshotCacheConfig::default().cache_path(Some(temp_dir.path()));
        let cache = SharedSnapshotFileCache::with_config(config)?;

        let content = Bytes::from("Shared cache auto-calc test");
        let returned_content_id = cache.store(&content)?;

        // Should return Some(ContentId) for enabled cache
        assert!(returned_content_id.is_some());
        let content_id = returned_content_id.unwrap();

        // Verify the returned ContentId matches what we would calculate manually
        let expected_content_id = calc_contentid(&content);
        assert_eq!(content_id, expected_content_id);

        // Verify we can retrieve the content using the returned ContentId
        let retrieved = cache.get(&content_id)?;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), content);

        Ok(())
    }

    #[test]
    fn test_empty_cache_path_disabled() -> Result<()> {
        use std::collections::BTreeMap;

        // Test with empty path
        let mut config = BTreeMap::new();
        config.insert("snapshot.cache-path", "");

        let cache_config = SnapshotCacheConfig::from_config(&config)?;
        assert!(!cache_config.is_enabled());
        assert!(cache_config.cache_path.is_none());

        // Test with whitespace-only path
        let mut config = BTreeMap::new();
        config.insert("snapshot.cache-path", "   ");

        let cache_config = SnapshotCacheConfig::from_config(&config)?;
        assert!(!cache_config.is_enabled());
        assert!(cache_config.cache_path.is_none());

        // Test that OptionalSnapshotFileCache handles this gracefully
        let mut cache = OptionalSnapshotFileCache::with_config(cache_config)?;
        assert!(!cache.is_enabled());

        let content = Bytes::from("This won't be cached");
        let content_id = calc_contentid(&content);

        // Should be no-op
        cache.store_with_content_id(content_id, &content)?;

        // Should return None
        let retrieved = cache.get(&content_id)?;
        assert!(retrieved.is_none());

        Ok(())
    }

    #[test]
    fn test_snapshot_caching_with_bonsai_changeset_id() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config = SnapshotCacheConfig::default().cache_path(Some(temp_dir.path()));
        let cache = SharedSnapshotFileCache::with_config(config)?;

        // Create a test CacheableSnapshot
        let snapshot = CacheableSnapshot {
            hg_parents: Parents::None,
            file_changes: vec![],
            author: "test_author".to_string(),
            time: 1234567890,
            tz: 0,
            bubble_id: None,
            labels: vec!["test".to_string()],
            cached: None,
        };

        // Create a test BonsaiChangesetId
        let changeset_id = BonsaiChangesetId::from_byte_array([0x42u8; 32]);

        // Store the snapshot
        cache.store_snapshot(changeset_id, &snapshot)?;

        // Verify it can be retrieved and matches the original snapshot
        let retrieved = cache.get_snapshot(&changeset_id)?;
        assert!(retrieved.is_some());
        let retrieved_snapshot = retrieved.unwrap();
        assert_eq!(retrieved_snapshot, snapshot);

        // Verify contains check works
        assert!(cache.contains_snapshot(&changeset_id)?);

        // Test with a different changeset ID that doesn't exist
        let other_changeset_id = BonsaiChangesetId::from_byte_array([0x43u8; 32]);
        assert!(!cache.contains_snapshot(&other_changeset_id)?);
        assert!(cache.get_snapshot(&other_changeset_id)?.is_none());

        Ok(())
    }

    #[test]
    fn test_snapshot_cbor_roundtrip() -> Result<()> {
        let snapshot = CacheableSnapshot {
            hg_parents: Parents::None,
            file_changes: vec![],
            author: "test_author".to_string(),
            time: 1234567890,
            tz: -28800, // PST timezone
            bubble_id: Some(std::num::NonZeroU64::new(42).unwrap()),
            labels: vec!["label1".to_string(), "label2".to_string()],
            cached: None,
        };
        let serialized = serde_cbor::to_vec(&snapshot)?;
        let deserialized: CacheableSnapshot = serde_cbor::from_slice(&serialized)?;
        assert_eq!(snapshot, deserialized);
        Ok(())
    }
}
