/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use taggederror::CommonMetadata;
use taggederror::IntentionalError;
use taggederror::Tagged;
use taggederror::TaggedError;

pub trait AnyhowEdenExt {
    /// Like AnyhowExt::common_metadata, except provides default metadata for known-Tagged Eden types.
    fn eden_metadata(&self) -> CommonMetadata;
}

impl AnyhowEdenExt for anyhow::Error {
    fn eden_metadata(&self) -> CommonMetadata {
        let mut metadata: CommonMetadata = Default::default();

        for cause in self.chain() {
            // Explicit metadata in error chain, created with AnyhowExt or .tagged()
            if let Some(e) = cause.downcast_ref::<TaggedError>() {
                metadata.merge(&e.metadata);
            }

            // Implicit metadata, types known to implement Tagged
            // Add your type here to avoid having to type .tagged() to wrap it
            if let Some(e) = cause.downcast_ref::<IntentionalError>() {
                metadata.merge(&e.metadata());
            }

            if metadata.complete() {
                break;
            }
        }
        metadata
    }
}

impl<T> AnyhowEdenExt for anyhow::Result<T> {
    fn eden_metadata(&self) -> CommonMetadata {
        if let Some(errref) = self.as_ref().err() {
            errref.eden_metadata()
        } else {
            Default::default()
        }
    }
}
