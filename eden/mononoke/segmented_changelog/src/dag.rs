/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use dag::{self, CloneData, InProcessIdDag, Location};
use stats::prelude::*;

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::idmap::IdMap;
use crate::read_only::ReadOnlySegmentedChangelog;
use crate::{segmented_changelog_delegate, SegmentedChangelog, StreamCloneData};

define_stats! {
    prefix = "mononoke.segmented_changelog.dag";
    location_to_changeset_id: timeseries(Sum),
}

// Note. The equivalent graph in the scm/lib/dag crate is `NameDag`.
pub struct Dag {
    pub(crate) iddag: InProcessIdDag,
    pub(crate) idmap: Arc<dyn IdMap>,
}

impl Dag {
    pub fn new(iddag: InProcessIdDag, idmap: Arc<dyn IdMap>) -> Self {
        Self { iddag, idmap }
    }
}

segmented_changelog_delegate!(Dag, |&self, ctx: &CoreContext| {
    ReadOnlySegmentedChangelog::new(&self.iddag, self.idmap.clone())
});
