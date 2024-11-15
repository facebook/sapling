/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;

use progress_model::Registry;
use serde::Deserialize;
use serde::Serialize;
use termwiz::surface::Change;

use crate::RenderingConfig;

#[derive(Serialize, Deserialize)]
struct IpcProgressBar {
    pub id: u64,
    pub topic: String,
    pub unit: String,
    pub total: u64,
    pub position: u64,
    pub parent_id: Option<u64>,
}

pub fn render(registry: &Registry, config: &RenderingConfig) -> Vec<Change> {
    let ipc = if let Some(ipc) = nodeipc::get_singleton() {
        ipc
    } else {
        tracing::trace!("nodeipc channel not available when rendering nodeipc progress bar");
        return Vec::new();
    };

    let progress_bars: Vec<_> = registry
        .list_progress_bar()
        .into_iter()
        .filter(|pb| config.delay.as_millis() == 0 || pb.since_creation() >= config.delay)
        .map(|pb| {
            let (position, total) = pb.position_total();
            IpcProgressBar {
                id: pb.id(),
                topic: pb.topic().to_owned(),
                unit: pb.unit().to_owned(),
                total,
                position,
                parent_id: pb.parent().map(|p| p.id()),
            }
        })
        .collect();

    if let Err(err) = ipc.send(HashMap::from([(
        "progress_bar_update".to_owned(),
        progress_bars,
    )])) {
        tracing::trace!("nodeipc send error on progress: {:?}", err);
    }

    Vec::new()
}
