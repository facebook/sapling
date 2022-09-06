/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::fs;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use configmodel::convert::ByteCount;
use configmodel::Config;
use configmodel::ConfigExt;
use minibytes::Bytes;
use thiserror::Error;
use types::Key;

use crate::datapack::DataPack;
use crate::datapack::DataPackVersion;
use crate::datastore::HgIdDataStore;
use crate::datastore::HgIdMutableDeltaStore;
use crate::datastore::StoreResult;
use crate::historypack::HistoryPack;
use crate::historypack::HistoryPackVersion;
use crate::historystore::HgIdHistoryStore;
use crate::historystore::HgIdMutableHistoryStore;
use crate::localstore::ExtStoredPolicy;
use crate::localstore::LocalStore;
use crate::localstore::StoreFromPath;
use crate::metadatastore::MetadataStore;
use crate::mutabledatapack::MutableDataPack;
use crate::mutablehistorypack::MutableHistoryPack;
use crate::mutablepack::MutablePack;
use crate::types::StoreKey;
use crate::LegacyStore;

#[derive(Copy, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub enum RepackLocation {
    Local,
    Shared,
}

#[derive(Copy, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub enum RepackKind {
    Incremental,
    Full,
}

pub trait ToKeys {
    fn to_keys(&self) -> Vec<Result<Key>>;
}

pub trait Repackable {
    fn delete(self) -> Result<()>;
    fn size(&self) -> u64;
}

fn repack_datapack(data_pack: &DataPack, mut_pack: &mut MutableDataPack) -> Result<()> {
    for k in data_pack.to_keys() {
        let key = k?;

        if let Some(chain) = data_pack.get_delta_chain(&key)? {
            for delta in chain.iter() {
                if mut_pack.contains(&StoreKey::hgid(delta.key.clone()))? {
                    break;
                }

                // If we managed to get a delta, the metadata must be present.
                match data_pack.get_meta(StoreKey::hgid(delta.key.clone()))? {
                    StoreResult::Found(meta) => mut_pack.add(&delta, &meta)?,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, Error)]
enum RepackFailure {
    #[error("Repack failure: {0:?}")]
    Total(Vec<(PathBuf, Error)>),

    #[error("Repack successful but with errors: {0:?}")]
    Partial(Vec<(PathBuf, Error)>),
}

/// Repack all pack files in the paths iterator. Once repacked, the repacked packs will be removed
/// from the filesystem.
fn repack_packs<T: MutablePack, U: LocalStore + Repackable + ToKeys + StoreFromPath>(
    paths: impl IntoIterator<Item = PathBuf> + Clone,
    mut mut_pack: T,
    repack_pack: impl Fn(&U, &mut T) -> Result<()>,
) -> Result<Option<PathBuf>> {
    if paths.clone().into_iter().count() <= 1 {
        if let Some(path) = paths.into_iter().next() {
            return Ok(Some(path));
        } else {
            return Ok(None);
        }
    }

    let mut repacked = vec![];
    let mut errors = vec![];

    for path in paths {
        match U::from_path(&path, ExtStoredPolicy::Use) {
            Ok(pack) => {
                if let Err(e) = repack_pack(&pack, &mut mut_pack) {
                    errors.push((path.clone(), e));
                } else {
                    repacked.push(path);
                }
            }
            Err(e) => {
                if let Some(e) = e.downcast_ref::<IoError>() {
                    if e.kind() == IoErrorKind::NotFound {
                        continue;
                    }
                }
                errors.push((path.clone(), e));
            }
        }
    }

    if repacked.is_empty() {
        if !errors.is_empty() {
            return Err(RepackFailure::Total(errors).into());
        } else {
            // Nothing to repack
            return Ok(None);
        }
    }

    let new_pack_path = mut_pack.close_pack()?.unwrap();
    let new_pack = U::from_path(&new_pack_path, ExtStoredPolicy::Use)?;

    let mut successfully_repacked = 0;
    for path in repacked {
        if *path != new_pack_path {
            let pack = match U::from_path(&path, ExtStoredPolicy::Use) {
                Ok(pack) => pack,
                Err(_e) => {
                    // We were about to remove this file, let's just ignore the failures to open
                    // it.
                    successfully_repacked += 1;
                    continue;
                }
            };

            let keys = pack
                .to_keys()
                .into_iter()
                .filter_map(|res| res.ok())
                .map(StoreKey::hgid)
                .collect::<Vec<_>>();
            let missing = new_pack.get_missing(&keys)?;

            if missing.is_empty() {
                let _ = pack.delete();
                successfully_repacked += 1;
            } else {
                errors.push((path.clone(), format_err!("{:?}", missing)));
            }
        } else {
            successfully_repacked += 1;
        }
    }

    if successfully_repacked == 0 {
        Err(RepackFailure::Total(errors).into())
    } else if !errors.is_empty() {
        Err(RepackFailure::Partial(errors).into())
    } else {
        Ok(Some(new_pack_path))
    }
}

fn repack_datapacks(
    paths: impl IntoIterator<Item = PathBuf> + Clone,
    outdir: &Path,
) -> Result<Option<PathBuf>> {
    let mut_pack = MutableDataPack::new(outdir, DataPackVersion::One);

    repack_packs(paths, mut_pack, repack_datapack)
}

fn repack_historypack(history_pack: &HistoryPack, mut_pack: &mut MutableHistoryPack) -> Result<()> {
    for k in history_pack.to_keys() {
        let key = k?;
        if let Some(hgid) = history_pack.get_node_info(&key)? {
            mut_pack.add(&key, &hgid)?;
        }
    }

    Ok(())
}

fn repack_historypacks(
    paths: impl IntoIterator<Item = PathBuf> + Clone,
    outdir: &Path,
) -> Result<Option<PathBuf>> {
    let mut_pack = MutableHistoryPack::new(outdir, HistoryPackVersion::One);

    repack_packs(paths, mut_pack, repack_historypack)
}

/// List all the pack files in the directory `dir` that ends with `extension`.
fn list_packs(dir: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut dirents = fs::read_dir(dir)?
        .filter_map(|e| match e {
            Err(_) => None,
            Ok(entry) => {
                let entrypath = entry.path();
                if entrypath.extension() == Some(extension.as_ref()) {
                    Some(entrypath.with_extension(""))
                } else {
                    None
                }
            }
        })
        .collect::<Vec<PathBuf>>();
    dirents.sort_unstable();
    Ok(dirents)
}

/// Select all the packs from `packs` that needs to be repacked during an incremental repack.
///
/// The filtering is fairly basic and is intended to reduce the fragmentation of pack files.
fn filter_incrementalpacks(
    packs: Vec<PathBuf>,
    extension: &str,
    config: &dyn Config,
) -> Result<Vec<PathBuf>> {
    // The overall maximum pack size.
    let max_pack_size: u64 = {
        if extension == "histpack" {
            config.get_or("repack", "maxhistpacksize", || {
                // Per 100MB of histpack size, the memory consumption is over 1GB,
                // thus repacking 4GB would need over 40GB of RAM.
                ByteCount::from(400 * 1024 * 1024)
            })?
        } else {
            config.get_or("repack", "maxdatapacksize", || {
                ByteCount::from(4 * 1024 * 1024 * 1024)
            })?
        }
    }
    .value();
    // The size limit for any individual pack.
    let size_limit: u64 = config
        .get_or("repack", "sizelimit", || ByteCount::from(100 * 1024 * 1024))?
        .value();
    // The maximum number of packs we want to have after repack (overrides `size_limit`).
    let max_packs: usize = config.get_or("repack", "maxpacks", || 50)?;

    let mut packssizes = packs
        .into_iter()
        .map(|p| {
            let size = p
                .with_extension(extension)
                .metadata()
                .and_then(|m| Ok(m.len()))
                .unwrap_or(u64::max_value());
            (p, size)
        })
        .collect::<Vec<(PathBuf, u64)>>();

    // Sort by file size in increasing order
    packssizes.sort_unstable_by(|a, b| a.1.cmp(&b.1));

    let mut num_packs = packssizes.len();
    let mut accumulated_sizes = 0;
    Ok(packssizes
        .into_iter()
        .take_while(|e| {
            if e.1 + accumulated_sizes > max_pack_size {
                return false;
            }

            if e.1 > size_limit && num_packs < max_packs {
                false
            } else {
                accumulated_sizes += e.1;
                num_packs -= 1;

                true
            }
        })
        .map(|e| e.0)
        .collect())
}

/// Fallback for `repack` for when no `ContentStore`/`MetadataStore` were passed in. Will simply
/// use the legacy code path to write the content of the packfiles to a packfile.
fn repack_no_store(path: PathBuf, kind: RepackKind, config: &dyn Config) -> Result<()> {
    let mut datapacks = list_packs(&path, "datapack")?;
    let mut histpacks = list_packs(&path, "histpack")?;

    if kind == RepackKind::Incremental {
        datapacks = filter_incrementalpacks(datapacks, "datapack", config)?;
        histpacks = filter_incrementalpacks(histpacks, "histpack", config)?;
    }

    let datapack_res = repack_datapacks(datapacks, &path).map(|_| ());
    let histpack_res = repack_historypacks(histpacks, &path).map(|_| ());

    datapack_res.and(histpack_res)
}

fn repack_datapack_to_contentstore(
    paths: Vec<PathBuf>,
    store: &Arc<dyn LegacyStore>,
    location: RepackLocation,
) -> Result<()> {
    let mut repacked = Vec::with_capacity(paths.len());
    let mut errors = vec![];

    let mut seen = HashSet::new();
    for path in paths {
        let pack = match DataPack::new(&path, ExtStoredPolicy::Use) {
            Ok(pack) => pack,
            Err(_) => continue,
        };

        let res = (|| -> Result<()> {
            for key in pack.to_keys() {
                let key = key?;
                if !seen.contains(&key) {
                    if let StoreResult::Found(content) = store.get(StoreKey::hgid(key.clone()))? {
                        match store.get_meta(StoreKey::hgid(key.clone()))? {
                            StoreResult::Found(meta) => {
                                store.add_pending(&key, Bytes::from(content), meta, location)?;
                                seen.insert(key);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(())
        })();

        match res {
            Ok(_) => repacked.push(path),
            Err(e) => errors.push((path, e)),
        }
    }

    if repacked.is_empty() {
        return Err(RepackFailure::Total(errors).into());
    }

    let new_packs = store
        .commit_pending(location)?
        .unwrap_or_else(|| vec![])
        .into_iter()
        .collect::<HashSet<PathBuf>>();

    for path in repacked {
        // TODO: This is a bit fragile as a bug in commit_pending not returning a path could lead
        // to data loss. A better return type would avoid this.
        if !new_packs.contains(&path) {
            match DataPack::new(&path, ExtStoredPolicy::Use) {
                Ok(pack) => pack.delete()?,
                Err(_) => continue,
            }
        }
    }

    if !errors.is_empty() {
        Err(RepackFailure::Partial(errors).into())
    } else {
        Ok(())
    }
}

fn repack_histpack_to_metadatastore(
    paths: Vec<PathBuf>,
    store: &MetadataStore,
    location: RepackLocation,
) -> Result<()> {
    let mut repacked = Vec::with_capacity(paths.len());
    let mut errors = vec![];

    for path in paths {
        let pack = match HistoryPack::new(&path) {
            Ok(pack) => pack,
            Err(_) => continue,
        };

        let res = (|| -> Result<()> {
            for key in pack.to_keys() {
                let key = key?;

                if let Some(nodeinfo) = store.get_node_info(&key)? {
                    store.add_pending(&key, nodeinfo, location)?;
                }
            }

            Ok(())
        })();

        match res {
            Ok(_) => repacked.push(path),
            Err(e) => errors.push((path, e)),
        }
    }

    if repacked.is_empty() {
        return Err(RepackFailure::Total(errors).into());
    }

    let new_packs = store
        .commit_pending(location)?
        .unwrap_or_else(|| vec![])
        .into_iter()
        .collect::<HashSet<PathBuf>>();

    for path in repacked {
        if !new_packs.contains(&path) {
            match HistoryPack::new(&path) {
                Ok(pack) => pack.delete()?,
                Err(_) => continue,
            }
        }
    }

    if !errors.is_empty() {
        Err(RepackFailure::Partial(errors).into())
    } else {
        Ok(())
    }
}

/// Read blobs and metadata contained in the packfiles from `path` and write them back to the
/// `stores`.
///
/// The primary goal of `repack` is to reduce the performance effect of having many packfiles on
/// disk. This is done by writing all the data (and metadata) of the several packfiles onto one.
///
/// The secondary goal of `repack` is for file format changes, packfile are for instance holding
/// LFS pointers, and by virtue of writing these pointers to a `ContentStore`, these will be
/// written to the `LfsStore` instead of to a packfile.
///
/// When `RepackKind::Incremental` is passed in, only a subset of the packfiles will be repacked in
/// order to minimize CPU cost.
///
/// When `stores` is None, a much dumber repack operation is performed, where only the primary goal
/// is fullfilled.
pub fn repack(
    path: PathBuf,
    stores: Option<(Arc<dyn LegacyStore>, Arc<MetadataStore>)>,
    kind: RepackKind,
    location: RepackLocation,
    config: &dyn Config,
) -> Result<()> {
    let (content, metadata) = match stores {
        Some((content, metadata)) => (content, metadata),
        None => return repack_no_store(path, kind, config),
    };

    let mut datapacks = list_packs(&path, "datapack")?;
    let mut histpacks = list_packs(&path, "histpack")?;

    if kind == RepackKind::Incremental {
        // We may be filtering out packfiles that contain LFS pointers, reducing the effectiveness
        // of the secondary goal of repack. To fully perform this secondary goal, a full repack
        // will be necessary, to keep incremental repacks simple.
        datapacks = filter_incrementalpacks(datapacks, "datapack", config)?;
        histpacks = filter_incrementalpacks(histpacks, "histpack", config)?;
    }

    if !datapacks.is_empty() {
        repack_datapack_to_contentstore(datapacks, &content, location)?;
    }

    if !histpacks.is_empty() {
        repack_histpack_to_metadatastore(histpacks, &metadata, location)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs::set_permissions;
    use std::fs::File;
    use std::fs::OpenOptions;
    use std::io::Write;

    use minibytes::Bytes;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::TempDir;
    use types::testutil::*;

    use super::*;
    use crate::datapack::tests::make_datapack;
    use crate::datastore::Delta;
    use crate::historypack::tests::get_nodes;
    use crate::historypack::tests::make_historypack;
    use crate::testutil::empty_config;

    #[test]
    fn test_repack_filter_incremental() -> Result<()> {
        let tempdir = TempDir::new()?;
        let get_packfile_path = |size: usize| tempdir.path().join(format!("{}.datapack", size));
        let get_packfile_paths = |sizes: &[usize]| {
            sizes
                .iter()
                .map(|size| get_packfile_path(*size))
                .collect::<Vec<PathBuf>>()
        };

        let packs: Vec<PathBuf> = [100, 200, 300, 400, 500]
            .iter()
            .map(|size| {
                let path = get_packfile_path(*size);
                let mut file = File::create(&path).unwrap();
                let bytes = vec![0; *size];
                file.write_all(&bytes).unwrap();
                path
            })
            .collect();

        let config = empty_config();
        assert_eq!(
            filter_incrementalpacks(packs.clone(), "datapack", &config)?,
            get_packfile_paths(&[100, 200, 300, 400, 500])
        );

        let config = {
            let mut config = empty_config();
            config.insert("repack.sizelimit".to_string(), "300".to_string());
            config
        };
        assert_eq!(
            filter_incrementalpacks(packs.clone(), "datapack", &config)?,
            get_packfile_paths(&[100, 200, 300])
        );

        let config = {
            let mut config = empty_config();
            config.insert("repack.maxdatapacksize".to_string(), "1k".to_string());
            config
        };
        assert_eq!(
            filter_incrementalpacks(packs.clone(), "datapack", &config)?,
            get_packfile_paths(&[100, 200, 300, 400])
        );

        let config = {
            let mut config = empty_config();
            config.insert("repack.maxhistpacksize".to_string(), "1k".to_string());
            config
        };
        assert!(filter_incrementalpacks(packs.clone(), "histpack", &config)?.is_empty());

        // We have 5 packs pre-repack and we want to make sure we have no more
        // than 2 packs post-repack.
        let config = {
            let mut config = empty_config();
            config.insert("repack.sizelimit".to_string(), "300".to_string());
            config.insert("repack.maxpacks".to_string(), "2".to_string());
            config
        };
        assert_eq!(
            filter_incrementalpacks(packs, "datapack", &config)?,
            get_packfile_paths(&[100, 200, 300, 400])
        );

        Ok(())
    }

    #[test]
    fn test_repack_no_datapack() {
        let tempdir = TempDir::new().unwrap();

        let newpath = repack_datapacks(vec![].into_iter(), tempdir.path());
        assert!(newpath.is_ok());
        let newpath = newpath.unwrap();
        assert_eq!(newpath, None);
    }

    #[test]
    fn test_repack_one_datapack() {
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![(
            Delta {
                data: Bytes::from(&[1u8, 2, 3, 4][..]),
                base: None,
                key: key("a", "1"),
            },
            Default::default(),
        )];

        let pack = make_datapack(&tempdir, &revisions);
        let newpath = repack_datapacks(
            vec![pack.base_path().to_path_buf()].into_iter(),
            tempdir.path(),
        );
        assert!(newpath.is_ok());
        let newpath2 = newpath.unwrap().unwrap();
        assert_eq!(newpath2.with_extension("datapack"), pack.pack_path());
        let datapack = DataPack::new(&newpath2, ExtStoredPolicy::Use);
        assert!(datapack.is_ok());
        let newpack = datapack.unwrap();
        assert_eq!(
            newpack
                .to_keys()
                .into_iter()
                .collect::<Result<Vec<Key>>>()
                .unwrap(),
            revisions
                .iter()
                .map(|d| d.0.key.clone())
                .collect::<Vec<Key>>()
        );
    }

    #[test]
    fn test_repack_multiple_datapacks() {
        let tempdir = TempDir::new().unwrap();
        let mut revisions = Vec::new();
        let mut paths = Vec::new();

        for i in 1..=2 {
            let base = key("a", &i.to_string());
            let rev = vec![
                (
                    Delta {
                        data: Bytes::from(&[1, 2, 3, 4][..]),
                        base: None,
                        key: base.clone(),
                    },
                    Default::default(),
                ),
                (
                    Delta {
                        data: Bytes::from(&[1, 2, 3, 4][..]),
                        base: Some(base),
                        key: key("a", &(100 + i).to_string()),
                    },
                    Default::default(),
                ),
            ];
            let pack = make_datapack(&tempdir, &rev);
            let path = pack.base_path().to_path_buf();
            revisions.push(rev);
            paths.push(path);
        }

        let newpath = repack_datapacks(paths.into_iter(), tempdir.path());
        assert!(newpath.is_ok());
        let newpack = DataPack::new(&newpath.unwrap().unwrap(), ExtStoredPolicy::Use).unwrap();
        assert_eq!(
            newpack
                .to_keys()
                .into_iter()
                .collect::<Result<Vec<Key>>>()
                .unwrap(),
            revisions
                .iter()
                .flatten()
                .map(|d| d.0.key.clone())
                .collect::<Vec<Key>>()
        );
    }

    #[test]
    fn test_repack_missing_files() {
        let tempdir = TempDir::new().unwrap();

        let paths = vec![PathBuf::from("foo.datapack"), PathBuf::from("bar.datapack")];
        let res = repack_datapacks(paths.clone().into_iter(), tempdir.path());

        assert!(res.unwrap().is_none());
    }

    #[test]
    fn test_repack_corrupted() {
        let tempdir = TempDir::new().unwrap();
        let mut revisions = Vec::new();
        let mut paths = Vec::new();

        for i in 1..=2 {
            let base = key("a", &i.to_string());
            let rev = vec![
                (
                    Delta {
                        data: Bytes::from(&[1, 2, 3, 4][..]),
                        base: None,
                        key: base.clone(),
                    },
                    Default::default(),
                ),
                (
                    Delta {
                        data: Bytes::from(&[1, 2, 3, 4][..]),
                        base: Some(base),
                        key: key("a", &(100 + i).to_string()),
                    },
                    Default::default(),
                ),
            ];
            let pack = make_datapack(&tempdir, &rev);
            let path = pack.base_path().to_path_buf();
            revisions.push(rev);
            paths.push(path);
        }

        let mut to_corrupt = paths.get(0).unwrap().clone();
        to_corrupt.set_extension("datapack");
        let mut perms = to_corrupt.metadata().unwrap().permissions();
        perms.set_readonly(false);
        set_permissions(to_corrupt.clone(), perms).unwrap();
        let mut file = OpenOptions::new()
            .write(true)
            .open(to_corrupt.clone())
            .unwrap();
        file.write_all(b"FOOBARBAZ").unwrap();
        drop(file);

        let res = repack_datapacks(paths.into_iter(), tempdir.path())
            .err()
            .unwrap();

        if let Some(RepackFailure::Partial(errors)) = res.downcast_ref() {
            assert_eq!(errors.iter().count(), 1);
            to_corrupt.set_extension("");
            assert!(errors.iter().find(|(p, _)| p == &to_corrupt).is_some());
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_repack_one_historypack() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let nodes = get_nodes(&mut rng);

        let pack = make_historypack(&tempdir, &nodes);
        let newpath = repack_historypacks(
            vec![pack.base_path().to_path_buf()].into_iter(),
            tempdir.path(),
        );
        assert!(newpath.is_ok());
        let newpack = HistoryPack::new(&newpath.unwrap().unwrap()).unwrap();

        for (ref key, _) in nodes.iter() {
            let response = newpack.get_node_info(key).unwrap().unwrap();
            assert_eq!(&response, nodes.get(key).unwrap());
        }
    }

    #[test]
    fn test_repack_multiple_historypack() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();
        let mut nodes = HashMap::new();
        let mut paths = Vec::new();

        for _ in 0..2 {
            let hgid = get_nodes(&mut rng);
            let pack = make_historypack(&tempdir, &hgid);
            let path = pack.base_path().to_path_buf();

            nodes.extend(hgid.into_iter());
            paths.push(path);
        }

        let newpath = repack_historypacks(paths.into_iter(), tempdir.path());
        assert!(newpath.is_ok());
        let newpack = HistoryPack::new(&newpath.unwrap().unwrap()).unwrap();

        for (key, _) in nodes.iter() {
            let response = newpack.get_node_info(&key).unwrap().unwrap();
            assert_eq!(&response, nodes.get(key).unwrap());
        }
    }
}
