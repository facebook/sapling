/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Blobstore;
use context::CoreContext;
use futures::future;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use itertools::Itertools;
use manifest::ManifestOps;
use manifest::ManifestOrderedOps;
use manifest::PathOrPrefix;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use vec1::vec1;
use vec1::Vec1;

use crate::path::BsmPath;
use crate::RootBasenameSuffixSkeletonManifest;

type MononokePath = Option<MPath>;

fn create_prefix(prefix: MononokePath, mut basename: MPathElement) -> MononokePath {
    basename.reverse();
    Some(MPath::from(basename).join(MPath::iter_opt(prefix.as_ref())))
}

fn custom_to_normal(path: MononokePath) -> Result<MononokePath> {
    path.map(|p| BsmPath::from_bsm_formatted_path(p).untransform())
        .transpose()
}

fn normal_to_custom(path: MononokePath) -> MononokePath {
    path.map(|p| BsmPath::transform(p).into_raw())
}

impl RootBasenameSuffixSkeletonManifest {
    /// Finds all files with given basenames in the given directories.
    pub async fn find_files_filter_basenames(
        &self,
        ctx: &CoreContext,
        blobstore: impl Blobstore + Clone + Sync + Send + 'static,
        prefixes: Vec<MononokePath>,
        basenames: Vec1<String>,
        ordered: Option<Option<MononokePath>>,
    ) -> Result<impl Stream<Item = Result<MononokePath>>> {
        let basenames = basenames.try_mapped(|b| MPathElement::new(b.into_bytes()))?;
        let prefixes = Vec1::try_from_vec(prefixes)
            .unwrap_or_else(|_| vec1![None])
            .into_iter()
            .cartesian_product(basenames)
            .map(|(prefix, basename)| PathOrPrefix::Prefix(create_prefix(prefix, basename)));

        let entries = match ordered {
            None => self
                .0
                .find_entries(ctx.clone(), blobstore, prefixes)
                .left_stream(),
            // The order returned is consistent, but not really lexicographical
            // TODO: Fix order
            Some(after) => self
                .0
                .find_entries_ordered(
                    ctx.clone(),
                    blobstore,
                    prefixes,
                    after.map(normal_to_custom),
                )
                .right_stream(),
        };
        Ok(entries.try_filter_map(|(path, entry)| {
            future::ready(
                entry
                    .into_leaf()
                    .map(|_| custom_to_normal(path))
                    .transpose(),
            )
        }))
    }
}
