/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct GitHubRepo {
    pub owner: String,
    pub name: String,
}
