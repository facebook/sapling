/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Result;
use anyhow::bail;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use context::CoreContext;
use samplingblob::SamplingBlobstore;
use samplingblob::SamplingHandler;

/// Once armed, fails every read of a stored Hg manifest, standing in for a
/// store whose Hg manifests were deleted. Reads are also counted, so a read
/// whose failure the caller ignores still shows. Arm it after setup: deriving
/// Hg changesets reads the parents' Hg manifests.
#[derive(Debug, Default)]
pub struct HgManifestReadBlocker {
    armed: AtomicBool,
    reads: AtomicUsize,
}

impl HgManifestReadBlocker {
    /// Wrap `inner` so that its Hg manifest reads go through a new blocker.
    pub fn wrap(inner: impl Blobstore + 'static) -> (Arc<dyn Blobstore>, Arc<Self>) {
        let blocker = Arc::new(Self::default());
        let blobstore = Arc::new(SamplingBlobstore::new(inner, blocker.clone()));
        (blobstore, blocker)
    }

    pub fn arm(&self) {
        self.armed.store(true, Ordering::Relaxed);
    }

    /// Reads of stored Hg manifests since the blocker was armed.
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    fn check(&self, key: &str) -> Result<()> {
        if self.armed.load(Ordering::Relaxed) && key.contains("hgmanifest.sha1.") {
            self.reads.fetch_add(1, Ordering::Relaxed);
            bail!("blocked read of stored Hg manifest {key}");
        }
        Ok(())
    }
}

impl SamplingHandler for HgManifestReadBlocker {
    fn sample_get(
        &self,
        _ctx: &CoreContext,
        key: &str,
        _value: Option<&BlobstoreGetData>,
    ) -> Result<()> {
        self.check(key)
    }

    fn sample_is_present(
        &self,
        _ctx: &CoreContext,
        key: &str,
        _value: &BlobstoreIsPresent,
    ) -> Result<()> {
        self.check(key)
    }
}

#[cfg(test)]
mod tests {
    use blobstore::BlobstoreBytes;
    use fbinit::FacebookInit;
    use memblob::Memblob;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_blocks_hg_manifest_reads_once_armed(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let (blobstore, blocker) = HgManifestReadBlocker::wrap(Memblob::default());
        let hg_key = "repo0000.hgmanifest.sha1.1111111111111111111111111111111111111111";
        let augmented_key =
            "repo0000.hgaugmentedmanifest.sha1.1111111111111111111111111111111111111111";
        for key in [hg_key, augmented_key] {
            blobstore
                .put(&ctx, key.to_string(), BlobstoreBytes::from_bytes("x"))
                .await?;
        }

        assert!(
            blobstore.get(&ctx, hg_key).await?.is_some(),
            "reads pass before arming"
        );
        blocker.arm();
        assert!(blobstore.get(&ctx, hg_key).await.is_err(), "get is blocked");
        assert!(
            blobstore.is_present(&ctx, hg_key).await.is_err(),
            "is_present is blocked"
        );
        assert!(
            blobstore.get(&ctx, augmented_key).await?.is_some(),
            "augmented manifests are still read"
        );
        assert_eq!(blocker.reads(), 2, "both blocked reads are counted");
        Ok(())
    }
}
