/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Plain data structures for repo tool manifest XML files,
//! derived from the [`manifest DTD`][manifest-dtd].
//!
//! These are minimal definitions for Sapling to support the
//! repo tool, hence are NOT intended as a full coverage.
//!
//! [manifest-dtd]: https://gerrit.googlesource.com/git-repo/+/HEAD/docs/manifest-format.md

use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct Manifest {
    pub remotes: BTreeMap<String, Remote>,
    pub default: Option<Default>,
    pub projects: BTreeMap<PathBuf, Project>,
}

#[derive(Debug, Default)]
pub struct Remote {
    pub name: String,
    pub alias: Option<String>,
    pub fetch: String,
    pub pushurl: Option<String>,
    pub review: Option<String>,
    pub revision: Option<String>,
}

#[derive(Debug, Default)]
pub struct Default {
    pub remote: Option<String>,
    pub revision: Option<String>,
    pub dest_branch: Option<String>,
    pub upstream: Option<String>,
}

#[derive(Debug, Default)]
pub struct Project {
    pub name: String,
    pub path: Option<PathBuf>,
    pub remote: Option<String>,
    pub revision: Option<String>,
    pub upstream: Option<String>,

    pub copyfiles: Vec<Copyfile>,
    pub linkfiles: Vec<Linkfile>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Default)]
pub struct Copyfile {
    pub src: PathBuf,
    pub dest: PathBuf,
}

#[derive(Debug, Default)]
pub struct Linkfile {
    pub src: PathBuf,
    pub dest: PathBuf,
}

#[derive(Debug)]
pub struct Annotation {
    pub name: String,
    pub value: String,
    pub keep: bool,
}
