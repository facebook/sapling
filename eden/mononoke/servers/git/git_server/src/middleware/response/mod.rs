/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod content_type;
#[cfg(fbcode_build)]
mod facebook;
pub mod ods3;
#[cfg(not(fbcode_build))]
mod oss;
