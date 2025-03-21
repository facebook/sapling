/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingArc;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::BookmarkUpdateLogRef;
use filestore::FilestoreConfigRef;
use repo_cross_repo::RepoCrossRepoRef;

// TODO(T182311609): try to use all refs instead of arcs
pub trait Repo = commit_transformation::Repo
    + BonsaiGitMappingRef
    + BonsaiHgMappingArc
    + BookmarkUpdateLogArc
    + BookmarkUpdateLogRef
    + Clone
    + FilestoreConfigRef
    + RepoCrossRepoRef
    + Send
    + Sync
    + 'static;
