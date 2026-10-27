/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Picks the bookmark that best represents a repo: the largest root content
//! manifest among the bookmarks that moved most recently.

pub mod measure;
pub mod recent_bookmarks;
