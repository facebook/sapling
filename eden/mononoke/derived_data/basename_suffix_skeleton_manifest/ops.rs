/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Ordering;

use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use futures::future;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use itertools::EitherOrBoth;
use manifest::After;
use manifest::AsyncManifest;
use manifest::ManifestOps;
use manifest::ManifestOrderedOps;
use manifest::PathOrPrefix;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use vec1::vec1;
use vec1::Vec1;

use crate::path::BssmPath;
use crate::RootBasenameSuffixSkeletonManifest;

type MononokePath = Option<MPath>;

fn normal_to_custom(path: MononokePath) -> MononokePath {
    path.map(|p| BssmPath::transform(p).into_raw())
}

enum BasenameOrSuffix {
    Basename(MPathElement),
    Suffix(MPathElement),
}

impl BasenameOrSuffix {
    fn key(&self) -> (&[u8], bool) {
        match self {
            Self::Basename(b) => (b.as_ref(), false),
            Self::Suffix(s) => (s.as_ref(), true),
        }
    }
    fn cmp(a: &Self, b: &Self) -> Ordering {
        a.key().cmp(&b.key())
    }
}

impl RootBasenameSuffixSkeletonManifest {
    /// Finds all files with given basenames in the given directories.
    pub async fn find_files_filter_basenames<'a>(
        &self,
        ctx: &'a CoreContext,
        blobstore: impl Blobstore + Clone + Sync + Send + 'static,
        prefixes: Vec<MononokePath>,
        basenames_and_suffixes: EitherOrBoth<Vec1<String>, Vec1<String>>,
        ordered: Option<Option<MononokePath>>,
    ) -> Result<impl Stream<Item = Result<MononokePath>> + 'a> {
        let (basenames, suffixes) = basenames_and_suffixes
            .map_any(Vec1::into_vec, Vec1::into_vec)
            .or_default();
        let prefixes: Vec1<PathOrPrefix> = Vec1::try_from_vec(prefixes)
            .unwrap_or_else(|_| vec1![None])
            .mapped(PathOrPrefix::Prefix);
        let mut all: Vec<BasenameOrSuffix> = basenames
            .into_iter()
            .map(|b| {
                Ok(BasenameOrSuffix::Basename(MPathElement::new(
                    b.into_bytes(),
                )?))
            })
            .chain(
                suffixes
                    .into_iter()
                    .map(|s| Ok(BasenameOrSuffix::Suffix(MPathElement::new(s.into_bytes())?))),
            )
            .collect::<Result<_>>()?;
        if ordered.is_some() {
            all.sort_unstable_by(BasenameOrSuffix::cmp);
        }
        let node = self.0.clone().load(ctx, &blobstore).await?;
        Ok(stream::iter(all.into_iter().map(anyhow::Ok))
            .and_then(move |bos| {
                cloned!(blobstore, ordered, prefixes, node);
                async move {
                    const FILE_ERR: &str = "Algorithmic error: Node should always be directory.";
                    use BasenameOrSuffix::*;
                    let nodes = match bos {
                        Basename(mut basename) => {
                            basename.reverse();
                            stream::iter(node.lookup(ctx, &blobstore, &basename).await?.map(
                                |entry| anyhow::Ok((basename, entry.into_dir().context(FILE_ERR)?)),
                            ))
                            .left_stream()
                        }
                        Suffix(mut suffix) => {
                            suffix.reverse();
                            // Why use async_stream::try_stream here to "forward" and not just
                            // return the stream? Because we need to take ownership of blobstore
                            // and suffix, otherwise we return a stream that references those values
                            // but does not own them, which causes a compilation error.
                            // I did not find a simpler way to do this.
                            cloned!(blobstore);
                            async_stream::try_stream! {
                                let mut s = node
                                    .list_prefix(ctx, &blobstore, suffix.as_ref())
                                    .await?;

                                while let Some((name, node)) = s.try_next().await? {
                                    let value = (name, node.into_tree().context(FILE_ERR)?);
                                    yield value;
                                }
                            }
                        }
                        .right_stream(),
                    };

                    Ok(nodes
                        .map_ok(move |(name, node)| {
                            let entries = match ordered.clone() {
                                None => node
                                    .find_entries(ctx.clone(), blobstore.clone(), prefixes.clone())
                                    .left_stream(),
                                Some(after) => {
                                    let after: After = after.map(normal_to_custom).into();
                                    // This will still traverse all distinct basenames even if we're
                                    // skipping a bunch. If this needs optimisation, we should skip
                                    // a suffix if we know all names starting with that suffix are
                                    // gonna be skipped anyway.
                                    let after = if after.skip(&name) {
                                        return stream::empty().left_stream();
                                    } else {
                                        after.enter_dir(&name)
                                    };
                                    node.find_entries_ordered(
                                        ctx.clone(),
                                        blobstore.clone(),
                                        prefixes.clone(),
                                        after,
                                    )
                                    .right_stream()
                                }
                            };
                            entries
                                .try_filter_map(|(path, entry)| {
                                    // No need to "untransform" path because we're already searching
                                    // from the reverse basename directory
                                    future::ok(entry.into_leaf().map(|_| path))
                                })
                                .right_stream()
                        })
                        .try_flatten())
                }
            })
            .try_flatten())
    }
}
