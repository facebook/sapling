/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utilities to generate reasonably looking stack of changesets
use anyhow::Error;
use anyhow::Result;
use blobrepo::save_bonsai_changesets;
use blobrepo::BlobRepo;
use blobstore::Storable;
use context::CoreContext;
use futures::future;
use futures::stream;
use futures::TryStreamExt;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileContents;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use rand::seq::SliceRandom;
use rand::Rng;
use rand_distr::Binomial;
use rand_distr::Uniform;
use std::collections::BTreeMap;

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

#[derive(Default, Debug, Clone, Copy)]
pub struct Size {
    pub width: usize,
    pub depth: usize,
    pub files: usize,
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

    pub async fn gen_stack(
        &mut self,
        ctx: CoreContext,
        repo: BlobRepo,
        rng: &mut impl Rng,
        settings: &GenSettings,
        parent: Option<ChangesetId>,
        changes_count: impl IntoIterator<Item = usize>,
    ) -> Result<ChangesetId, Error> {
        let mut parents: Vec<_> = parent.into_iter().collect();
        let mut changesets = Vec::new();
        async move {
            let blobstore = repo.blobstore();
            let store_changes = stream::FuturesUnordered::new();
            for changes_size in changes_count {
                // generate file changes
                let mut file_changes = BTreeMap::new();
                while file_changes.len() < changes_size {
                    let (path, content) = self.gen_change(rng, settings, Vec::new());
                    match content {
                        None => {
                            file_changes.insert(path, FileChange::Deletion);
                        }
                        Some(content) => {
                            let content = FileContents::new_bytes(content);
                            let size = content.size();
                            let blob = content.into_blob();
                            let id = *blob.id();
                            store_changes.push(blob.store(&ctx, blobstore));
                            file_changes.insert(
                                path,
                                FileChange::tracked(id, FileType::Regular, size as u64, None),
                            );
                        }
                    }
                }
                // generate changeset
                let bonsai = BonsaiChangesetMut {
                    parents: std::mem::take(&mut parents),
                    author: "author".to_string(),
                    author_date: DateTime::from_timestamp(0, 0).unwrap(),
                    committer: None,
                    committer_date: None,
                    message: "message".to_string(),
                    extra: Default::default(),
                    file_changes: file_changes.into(),
                    is_snapshot: false,
                }
                .freeze()
                .expect("generated bonsai failed to freeze");
                parents.push(bonsai.get_changeset_id());
                changesets.push(bonsai);
            }

            match parents.into_iter().next() {
                None => Err(Error::msg("empty changes iterator")),
                Some(csid) => {
                    store_changes.try_for_each(|_| future::ok(())).await?;
                    save_bonsai_changesets(changesets, ctx, &repo).await?;
                    Ok(csid)
                }
            }
        }
        .await
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
                let dirs = self.dirs.keys().collect::<Vec<_>>();
                dirs.choose(rng)
                    .map_or_else(|| gen_filename(rng), |&d| d.clone())
            };
            prefix.push(dirname.clone());
            self.dirs
                .entry(dirname)
                .or_insert_with(Self::new)
                .gen_change(rng, settings, prefix)
        } else {
            let (filename, new) = if rng.gen_bool(settings.p_file_create) {
                (gen_filename(rng), true)
            } else {
                let files = self.files.keys().collect::<Vec<_>>();
                files
                    .choose(rng)
                    .map_or_else(|| (gen_filename(rng), true), |&k| (k.clone(), false))
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
