/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use futures::future;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use itertools::EitherOrBoth;
use manifest::Entry as ManifestEntry;
use mononoke_types::path::MPath;
use mononoke_types::MPathElement;
use repo_blobstore::RepoBlobstoreRef;
use vec1::Vec1;

use super::to_vec1;
use super::ChangesetContext;
use super::ChangesetFileOrdering;
use crate::errors::MononokeError;
use crate::MononokeRepo;

/// A context object representing a query to a particular commit in a repo.
impl<R: MononokeRepo> ChangesetContext<R> {
    pub async fn find_files_unordered(
        &self,
        prefixes: Option<Vec<MPath>>,
        basenames: Option<Vec<String>>,
    ) -> Result<impl Stream<Item = Result<MPath, MononokeError>> + '_, MononokeError> {
        self.find_files(
            prefixes,
            basenames,
            // None for basename_suffixes
            None,
            ChangesetFileOrdering::Unordered,
        )
        .await
    }

    /// Find files after applying filters on the prefix and basename.
    /// A files is returned if the following conditions hold:
    /// - `prefixes` is None, or there is an element of `prefixes` such that the
    ///   element is a prefix of the file path.
    /// - the basename of the file path is in `basenames`, or there is a string
    ///   in `basename_suffixes` that is a suffix of the basename of the file,
    ///   or both `basenames` and `basename_suffixes` are None.
    ///
    /// The order that files are returned is based on the parameter `ordering`.
    /// To continue a paginated query, use the parameter `ordering`.
    pub async fn find_files(
        &self,
        prefixes: Option<Vec<MPath>>,
        basenames: Option<Vec<String>>,
        basename_suffixes: Option<Vec<String>>,
        ordering: ChangesetFileOrdering,
    ) -> Result<impl Stream<Item = Result<MPath, MononokeError>> + '_, MononokeError> {
        let basenames_and_suffixes = match (to_vec1(basenames), to_vec1(basename_suffixes)) {
            (None, None) => None,
            (Some(basenames), None) => Some(EitherOrBoth::Left(basenames)),
            (None, Some(suffixes)) => Some(EitherOrBoth::Right(suffixes)),
            (Some(basenames), Some(suffixes)) => Some(EitherOrBoth::Both(basenames, suffixes)),
        };
        Ok(match basenames_and_suffixes {
            Some(basenames_and_suffixes)
                if justknobs::eval(
                    "scm/mononoke:enable_bssm_v3",
                    None,
                    Some(self.repo_ctx().name()),
                )
                .unwrap_or_default()
                    && (!basenames_and_suffixes.has_right()
                        || justknobs::eval(
                            "scm/mononoke:enable_bssm_v3_suffix_query",
                            None,
                            Some(self.repo_ctx().name()),
                        )
                        .unwrap_or_default()) =>
            {
                self.find_files_with_bssm_v3(prefixes, basenames_and_suffixes, ordering)
                    .await?
                    .boxed()
            }
            basenames_and_suffixes => {
                let (basenames, basename_suffixes) = basenames_and_suffixes
                    .map_or((None, None), |b| b.map_any(Some, Some).or_default());
                self.find_files_without_bssm(
                    to_vec1(prefixes),
                    basenames,
                    basename_suffixes,
                    ordering,
                )
                .await?
                .boxed()
            }
        })
    }

    pub async fn find_files_with_bssm_v3(
        &self,
        prefixes: Option<Vec<MPath>>,
        basenames_and_suffixes: EitherOrBoth<Vec1<String>, Vec1<String>>,
        ordering: ChangesetFileOrdering,
    ) -> Result<impl Stream<Item = Result<MPath, MononokeError>> + '_, MononokeError> {
        Ok(self
            .root_bssm_v3_directory_id()
            .await?
            .find_files_filter_basenames(
                self.ctx(),
                self.repo_ctx().repo().repo_blobstore().clone(),
                prefixes.unwrap_or_else(Vec::new).into_iter().collect(),
                basenames_and_suffixes,
                match ordering {
                    ChangesetFileOrdering::Unordered => None,
                    ChangesetFileOrdering::Ordered { after } => Some(after),
                },
            )
            .await
            .map_err(MononokeError::from)?
            .map(|r| match r {
                Ok(p) => Ok(p),
                Err(err) => Err(MononokeError::from(err)),
            }))
    }

    pub(crate) async fn find_files_without_bssm(
        &self,
        prefixes: Option<Vec1<MPath>>,
        basenames: Option<Vec1<String>>,
        basename_suffixes: Option<Vec1<String>>,
        ordering: ChangesetFileOrdering,
    ) -> Result<impl Stream<Item = Result<MPath, MononokeError>>, MononokeError> {
        // First, find the entries, and filter by file prefix.

        let mpaths = if justknobs::eval(
            "scm/mononoke:mononoke_api_find_files_use_skeleton_manifests_v2",
            None,
            Some(self.repo_ctx().name()),
        )? {
            let entries = self.find_entries_v2(prefixes, ordering).await?;
            entries
                .try_filter_map(|(path, entry)| async move {
                    match (path.into_optional_non_root_path(), entry) {
                        (Some(mpath), ManifestEntry::Leaf(_)) => Ok(Some(mpath)),
                        _ => Ok(None),
                    }
                })
                .left_stream()
        } else {
            let entries = self.find_entries(prefixes, ordering).await?;
            entries
                .try_filter_map(|(path, entry)| async move {
                    match (path.into_optional_non_root_path(), entry) {
                        (Some(mpath), ManifestEntry::Leaf(_)) => Ok(Some(mpath)),
                        _ => Ok(None),
                    }
                })
                .right_stream()
        };

        // Now, construct a set of basenames to include.
        // These basenames are of type MPathElement rather than being strings.
        let basenames_as_mpath_elements_set = match basenames {
            Some(basenames) => Some(
                basenames
                    .into_iter()
                    .map(|basename| MPathElement::new(basename.into()))
                    .collect::<Result<HashSet<_>, _>>()
                    .map_err(MononokeError::from)?,
            ),
            None => None,
        };

        // Now, filter by basename. We use "left_stream" and "right_stream" to
        // satisfy the type checker, because filtering a stream creates a
        // different "type". Using left and right streams creates an Either type
        // which satisfies the type checker.
        let mpaths = match (basenames_as_mpath_elements_set, basename_suffixes) {
            // If basenames and suffixes are provided, include basenames in
            // the set basenames_as_mpath_elements_set as well as basenames
            // with a suffix in basename_suffixes.
            (Some(basenames_as_mpath_elements_set), Some(basename_suffixes)) => mpaths
                .try_filter(move |mpath| {
                    let basename = mpath.basename();
                    future::ready(
                        basenames_as_mpath_elements_set.contains(basename)
                            || basename_suffixes
                                .iter()
                                .any(|suffix| basename.has_suffix(suffix.as_bytes())),
                    )
                })
                .left_stream()
                .left_stream(),
            // If no suffixes are provided, only match on basenames that are
            // in the set.
            (Some(basenames_as_mpath_elements_set), None) => mpaths
                .try_filter(move |mpath| {
                    future::ready(basenames_as_mpath_elements_set.contains(mpath.basename()))
                })
                .left_stream()
                .right_stream(),
            (None, Some(basename_suffixes)) =>
            // If only suffixes are provided, match on basenames that have a
            // suffix in basename_suffixes.
            {
                mpaths
                    .try_filter(move |mpath| {
                        let basename = mpath.basename();
                        future::ready(
                            basename_suffixes
                                .iter()
                                .any(|suffix| basename.has_suffix(suffix.as_bytes())),
                        )
                    })
                    .right_stream()
                    .left_stream()
            }
            // Otherwise, there are no basename filters, so do not filter.
            (None, None) => mpaths.right_stream().right_stream(),
        };

        Ok(mpaths.map_ok(MPath::from).map_err(MononokeError::from))
    }
}
