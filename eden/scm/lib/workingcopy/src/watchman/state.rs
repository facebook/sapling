/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use serde::Deserialize;
use watchman_client::prelude::*;

use crate::filesystem::PendingChangeResult;

query_result_type! {
    pub struct StatusQuery {
        name: NameField,
        exists: ExistsField,
    }
}

pub struct WatchmanState {}

impl WatchmanState {
    pub fn new(_state_reader: impl std::io::Read) -> Self {
        WatchmanState {}
    }

    pub fn get_clock(&self) -> Option<Clock> {
        None
    }

    pub fn merge(&mut self, _result: QueryResult<StatusQuery>) {}

    pub fn persist(&self, _state_writer: impl std::io::Write) {}

    pub fn pending_changes(&self) -> impl Iterator<Item = Result<PendingChangeResult>> {
        (vec![]).into_iter()
    }
}
