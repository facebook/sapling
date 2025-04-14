/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repo Event Publisher.
//!
//! Responsible for publishing repo events (e.g. bookmark updates, tag updates, etc.)
//! and allowing interested parties to subscribe to them.
use std::hash::Hash;

/// The core Repo Event Publisher facet.
#[facet::facet]
#[derive(Hash, PartialEq, Eq, Clone)]
pub struct RepoEventPublisher {}
