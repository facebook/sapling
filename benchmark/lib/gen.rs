/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Utilities to generate reasonably looking stack of changesets
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobstore::Storable;
use context::CoreContext;
use failure_ext::{err_msg, Error};
use futures::{future, stream, Future, Stream};
use futures_ext::FutureExt;
use mononoke_types::{
    BlobstoreValue, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, FileContents, FileType,
    MPath, MPathElement,
};
use rand::{seq::SliceRandom, Rng};
use rand_distr::{Binomial, Uniform};
use std::{collections::BTreeMap, convert::TryFrom, iter::FromIterator};

#[derive(Clone, Copy)]
pub struct GenSettings {
    /// probablity of descending one level deeper when generating change
    pub p_dir_descend: f64,
    /// probablity to create directory or reuse existing when descending manifest
    pub p_dir_create: f64,
    /// probability to create file or modify existing
    pub p_file_create: f64,
    /// probability to delete file instead of modifying
    pub p_file_delete: f64,
}

impl Default for GenSettings {
    fn default() -> Self {
        Self {
            p_dir_descend: 0.7,
            p_dir_create: 0.2,
            p_file_create: 0.3,
            p_file_delete: 0.1,
        }
    }
}

pub struct GenManifest {
    dirs: BTreeMap<MPathElement, Box<GenManifest>>,
    files: BTreeMap<MPathElement, String>,
}

#[derive(Debug, Clone, Copy)]
pub struct Size {
    pub width: usize,
    pub depth: usize,
    pub files: usize,
}

impl Default for Size {
    fn default() -> Self {
        Self {
            width: 0,
            depth: 0,
            files: 0,
        }
    }
}

impl GenManifest {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            dirs: BTreeMap::new(),
            files: BTreeMap::new(),
        })
    }

    pub fn size(&self) -> Size {
        let children_size = self.dirs.iter().map(|(_, child)| child.size()).fold(
            Size::default(),
            |acc_size, size| Size {
                files: acc_size.files + size.files,
                depth: std::cmp::max(acc_size.depth, size.depth),
                width: std::cmp::max(acc_size.width, size.width),
            },
        );
        Size {
            depth: children_size.depth + 1,
            width: std::cmp::max(children_size.width, self.files.len() + self.dirs.len()),
            files: children_size.files + self.files.len(),
        }
    }

    pub fn gen_stack(
        &mut self,
        ctx: CoreContext,
        repo: BlobRepo,
        rng: &mut impl Rng,
        settings: &GenSettings,
        parent: Option<ChangesetId>,
        changes_count: impl IntoIterator<Item = usize>,
    ) -> impl Future<Item = ChangesetId, Error = Error> {
        let mut parents: Vec<_> = parent.into_iter().collect();
        let mut changesets = Vec::new();
        let mut store_changes = stream::FuturesUnordered::new();
        for changes_size in changes_count {
            // generate file changes
            let mut file_changes = BTreeMap::new();
            while file_changes.len() < changes_size {
                let (path, content) = self.gen_change(rng, settings, Vec::new());
                match content {
                    None => {
                        file_changes.insert(path, None);
                    }
                    Some(content) => {
                        let content = FileContents::new_bytes(content);
                        let size = content.size();
                        let blob = content.into_blob();
                        let id = *blob.id();
                        store_changes.push(blob.store(ctx.clone(), &repo.get_blobstore()));
                        file_changes.insert(
                            path,
                            Some(FileChange::new(id, FileType::Regular, size as u64, None)),
                        );
                    }
                }
            }
            // generate changeset
            let bonsai = BonsaiChangesetMut {
                parents: std::mem::replace(&mut parents, Vec::new()),
                author: "author".to_string(),
                author_date: DateTime::from_timestamp(0, 0).unwrap(),
                committer: None,
                committer_date: None,
                message: "message".to_string(),
                extra: BTreeMap::new(),
                file_changes,
            }
            .freeze()
            .expect("generated bonsai failed to freeze");
            parents.push(bonsai.get_changeset_id());
            changesets.push(bonsai);
        }

        match parents.into_iter().next() {
            None => future::err(err_msg("empty changes iterator")).left_future(),
            Some(csid) => store_changes
                .for_each(|_| future::ok(()))
                .and_then(move |_| save_bonsai_changesets(changesets, ctx, repo))
                .map(move |_| csid)
                .right_future(),
        }
    }

    fn gen_change(
        &mut self,
        rng: &mut impl Rng,
        settings: &GenSettings,
        mut prefix: Vec<MPathElement>,
    ) -> (MPath, Option<String>) {
        if rng.gen_bool(settings.p_dir_descend) {
            let dirname = if rng.gen_bool(settings.p_dir_create) {
                gen_filename(rng)
            } else {
                let dirs = Vec::from_iter(self.dirs.keys());
                dirs.choose(rng)
                    .map(|&d| d.clone())
                    .unwrap_or_else(|| gen_filename(rng))
            };
            prefix.push(dirname.clone());
            self.dirs
                .entry(dirname)
                .or_insert_with(|| Self::new())
                .gen_change(rng, settings, prefix)
        } else {
            let (filename, new) = if rng.gen_bool(settings.p_file_create) {
                (gen_filename(rng), true)
            } else {
                let files = Vec::from_iter(self.files.keys());
                files
                    .choose(rng)
                    .map(|&k| (k.clone(), false))
                    .unwrap_or_else(|| (gen_filename(rng), true))
            };
            prefix.push(filename.clone());
            let data = if !new && rng.gen_bool(settings.p_file_delete) {
                self.files.remove(&filename);
                None
            } else {
                let data = gen_ascii(16, rng);
                self.files.insert(filename, data.clone());
                Some(data)
            };
            (MPath::try_from(prefix).expect("prefix is empty"), data)
        }
    }
}

fn gen_ascii(len: usize, rng: &mut impl Rng) -> String {
    let chars = b"_abcdefghijklmnopqrstuvwxyz";
    let bytes = rng
        .sample_iter(&Uniform::from(0..chars.len()))
        .take(len)
        .map(|i| chars[i])
        .collect();
    String::from_utf8(bytes).expect("ascii conversion failed")
}

fn gen_filename(rng: &mut impl Rng) -> MPathElement {
    let len = rng.sample(&Binomial::new(20, 0.3).expect("Binomial::new failed")) as usize;
    MPathElement::new(gen_ascii(len + 3, rng).into()).expect("failed to create mpath element")
}
