/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crossbeam::channel::Sender;
use types::fetch_mode::FetchMode;
use types::Key;

use super::metrics::TreeStoreFetchMetrics;
use super::types::StoreTree;
use super::types::TreeAttributes;
use crate::scmstore::fetch::CommonFetchState;
use crate::scmstore::fetch::FetchErrors;
use crate::scmstore::KeyFetchError;

pub struct FetchState {
    pub(crate) common: CommonFetchState<StoreTree>,

    /// Errors encountered during fetching.
    errors: FetchErrors,

    /// Track fetch metrics,
    metrics: TreeStoreFetchMetrics,
}

impl FetchState {
    pub(crate) fn new(
        keys: impl IntoIterator<Item = Key>,
        attrs: TreeAttributes,
        found_tx: Sender<Result<(Key, StoreTree), KeyFetchError>>,
        fetch_mode: FetchMode,
    ) -> Self {
        FetchState {
            common: CommonFetchState::new(keys, attrs, found_tx, fetch_mode),
            errors: FetchErrors::new(),
            metrics: TreeStoreFetchMetrics::default(),
        }
    }
}
