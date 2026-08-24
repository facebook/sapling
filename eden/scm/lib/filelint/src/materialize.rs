/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::anyhow;
use anyhow::bail;
use blob::Blob;
use filewalk::FileResult;
use filewalk::WalkOptions;
use filewalk::fetch_file_nodes;
use manifest::FileMetadata;
use manifest::FileType;
use slex::Batch;
use slex::Items;
use slex::Work as SlexWork;
use slex::WorkOptions;
use smallvec::SmallVec;
use storemodel::FileStore;
use types::Blake3;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPathBuf;
use vfs::UpdateFlag;
use vfs::VFS;
use vfs::VfsBatchError;
use vfs::Work;

use crate::CacheKey;
use crate::LintCache;

const VFS_WORKERS: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializeFile {
    pub source_path: RepoPathBuf,
    pub hgid: HgId,
    pub file_type: FileType,
    pub destination: RepoPathBuf,
}

/// A compact identity for comparing materialized content after an external tool runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentFingerprint {
    pub size: usize,
    pub blake3: Blake3,
}

impl ContentFingerprint {
    pub fn from_blob(data: &Blob) -> Self {
        Self {
            size: data.len(),
            blake3: data.blake3(),
        }
    }
}

type Destinations = SmallVec<[(UpdateFlag, RepoPathBuf); 1]>;
type DestinationMap = HashMap<Key, Destinations>;

/// Fetch explicit file nodes and write them to their requested destinations,
/// returning each written destination's content fingerprint.
///
/// Content is fetched once per unique source node and streamed into bounded
/// VFS writes. Symlinks and submodules are skipped. Callers are responsible
/// for dropping oversized files beforehand (see aux data prefiltering).
pub fn materialize_files(
    output_root: PathBuf,
    file_store: &Arc<dyn FileStore>,
    files: Vec<MaterializeFile>,
    walk_options: WalkOptions,
) -> anyhow::Result<HashMap<RepoPathBuf, ContentFingerprint>> {
    std::fs::create_dir_all(&output_root)
        .with_context(|| format!("creating file output root `{}`", output_root.display()))?;

    let mut destinations = DestinationMap::new();
    let mut destination_paths = HashSet::new();
    for file in files {
        add_destination(&mut destinations, &mut destination_paths, file)?;
    }

    let file_nodes = destinations
        .iter()
        .map(|(key, destinations)| -> anyhow::Result<_> {
            let update_flag = destinations
                .first()
                .with_context(|| {
                    format!(
                        "file node {}@{} has no materialization destinations",
                        key.path, key.hgid
                    )
                })?
                .0;
            Ok((
                key.path.clone(),
                FileMetadata {
                    hgid: key.hgid,
                    file_type: manifest_file_type(update_flag),
                    ignore_unless_conflict: false,
                },
            ))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let work_items = fetch_file_nodes(
        Items::ready(file_nodes),
        file_store,
        FetchContext::sapling_default(),
        walk_options,
    )
    .map_batch(move |batch| match batch {
        Ok(files) => prepare_work_batch(files, &destinations),
        Err(err) => Err(VfsBatchError::Batch(err)),
    });

    let vfs = VFS::new(output_root)?;
    let mut written_files = HashMap::new();
    let mut first_error = None;
    for batch in vfs.batch_items(VFS_WORKERS, work_items).into_batches() {
        match batch {
            Ok(batch) => {
                for work in batch {
                    match work {
                        Work::Write(path, data, ..) => {
                            written_files.insert(path, ContentFingerprint::from_blob(&data));
                        }
                        work if first_error.is_none() => {
                            first_error = Some(anyhow!(
                                "unexpected completed file materialization work: {work:?}"
                            ));
                        }
                        work => {
                            tracing::debug!(?work, "skipping unexpected work after error")
                        }
                    }
                }
            }
            Err(err) if first_error.is_none() => first_error = Some(err.into_error()),
            Err(err) => {
                tracing::debug!("skipping additional error: {:#}", err.into_error())
            }
        }
    }
    if let Some(err) = first_error {
        return Err(err);
    }

    Ok(written_files)
}

/// Comparison results, identified by staged destination path (relative to
/// the comparison root), not repository source path. Changed files carry
/// the fingerprint of their new content so callers can compare again after
/// running another tool over the same staged files.
#[derive(Debug, Default)]
pub struct CompareResult {
    pub changed_files: Vec<(RepoPathBuf, ContentFingerprint)>,
    pub oversized_files: Vec<RepoPathBuf>,
}

enum CompareOutcome {
    Oversized(RepoPathBuf),
    Compared {
        changed: Option<(RepoPathBuf, ContentFingerprint)>,
        clean_key: CacheKey,
    },
}

/// A staged linter output to compare with its original content.
///
/// Fields follow [`MaterializeFile`]'s source-before-destination order so
/// callers translating between the two cannot silently swap the paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompareFile {
    /// Repository path the staged file was materialized from.
    pub source_path: RepoPathBuf,
    /// Path of the staged file relative to the comparison root.
    pub destination: RepoPathBuf,
    /// Fingerprint of the content before linting.
    pub fingerprint: ContentFingerprint,
}

/// Compare materialized files with their original fingerprints in parallel.
///
/// Since lint fixes are idempotent, every non-oversized output is lint
/// clean regardless of whether it changed, so its fingerprint is recorded in
/// `cache` under `epoch`. Cache write failures are logged, not fatal. Staged
/// files must exist: linters are expected to rewrite in place, so a
/// missing output fails the comparison.
pub fn compare_files(
    output_root: PathBuf,
    files: Vec<CompareFile>,
    cache: Option<&mut LintCache>,
    epoch: &[u8],
    max_file_size: usize,
) -> anyhow::Result<CompareResult> {
    let vfs = VFS::new(output_root)?;
    let epoch = epoch.to_vec();
    let outcomes = SlexWork::try_map(
        WorkOptions::new().max_workers(VFS_WORKERS),
        Items::ready(files),
        move |file| -> anyhow::Result<CompareOutcome> {
            let CompareFile {
                source_path,
                destination,
                fingerprint,
            } = file;
            if vfs
                .metadata(&destination)
                .with_context(|| format!("reading staged file metadata `{destination}`"))?
                .len()
                > max_file_size as u64
            {
                return Ok(CompareOutcome::Oversized(destination));
            }
            let data = vfs
                .read(&destination)
                .with_context(|| format!("reading staged file `{destination}`"))?;
            if data.len() > max_file_size {
                // The metadata check above is not atomic with the read; a
                // file that grew in between is still treated as oversized.
                return Ok(CompareOutcome::Oversized(destination));
            }
            let output = ContentFingerprint::from_blob(&Blob::Bytes(data));
            let clean_key = LintCache::key(&source_path, &output.blake3, &epoch);
            Ok(CompareOutcome::Compared {
                changed: (output != fingerprint).then_some((destination, output)),
                clean_key,
            })
        },
    )
    .into_iter()
    .collect::<anyhow::Result<Vec<_>>>()?;

    let mut result = CompareResult::default();
    let mut clean_keys = Vec::new();
    for outcome in outcomes {
        match outcome {
            CompareOutcome::Oversized(path) => result.oversized_files.push(path),
            CompareOutcome::Compared { changed, clean_key } => {
                result.changed_files.extend(changed);
                clean_keys.push(clean_key);
            }
        }
    }
    if let Some(cache) = cache {
        if let Err(err) = cache.record(clean_keys) {
            tracing::warn!(?err, "error recording lint clean-content cache entries");
        }
    }
    Ok(result)
}

/// Outcome of dropping unlintable file versions before any content fetch.
#[derive(Debug, Default)]
pub struct PrefilterResult {
    /// Versions that still need linting.
    pub keep: Vec<Key>,
    /// Number of versions dropped because their content exceeds the size limit.
    pub oversized_files: usize,
    /// Number of versions dropped because they are recorded as lint clean.
    pub clean_files: usize,
}

/// Filter candidate file versions by size and format history using batched
/// aux metadata.
///
/// Aux data describes content without fetching it, so oversized files and
/// versions recorded as lint clean under `epoch` are dropped before any
/// content transfer.
pub fn prefilter_files(
    file_store: &Arc<dyn FileStore>,
    files: Vec<Key>,
    mut cache: Option<&LintCache>,
    epoch: &[u8],
    max_file_size: usize,
) -> anyhow::Result<PrefilterResult> {
    let mut result = PrefilterResult::default();
    for entry in file_store.get_aux_iter(FetchContext::sapling_default(), files)? {
        let (key, aux) = entry.context("fetching aux data for lint prefilter")?;
        if aux.total_size > max_file_size as u64 {
            result.oversized_files += 1;
            continue;
        }
        // The cache only skips work, so read failures (ex. a log corrupted
        // mid-rotation) stop the lookups instead of failing the lint run,
        // matching how an unopenable cache disables itself.
        let clean = match cache
            .map(|cache| cache.contains(&LintCache::key(&key.path, &aux.blake3, epoch)))
        {
            Some(Ok(clean)) => clean,
            Some(Err(err)) => {
                tracing::warn!(?err, "error reading lint clean-content cache; ignoring it");
                cache = None;
                false
            }
            None => false,
        };
        if clean {
            result.clean_files += 1;
        } else {
            result.keep.push(key);
        }
    }
    Ok(result)
}

/// Fan each fetched source node out to its requested VFS writes.
fn prepare_work_batch(
    files: Batch<FileResult>,
    destinations: &DestinationMap,
) -> Result<Vec<Work>, VfsBatchError> {
    let mut work = Vec::new();
    for file in files {
        let key = Key::new(file.path, file.hgid);
        let Some(destinations) = destinations.get(&key) else {
            return Err(VfsBatchError::Batch(anyhow!(
                "file fetch returned unrequested key {}@{}",
                key.path,
                key.hgid
            )));
        };
        work.extend(destinations.iter().map(|(update_flag, path)| {
            Work::Write(path.clone(), file.data.clone(), *update_flag, None)
        }));
    }
    Ok(work)
}

/// Deduplicate source fetches while rejecting ambiguous destination writes.
fn add_destination(
    destinations: &mut DestinationMap,
    destination_paths: &mut HashSet<RepoPathBuf>,
    file: MaterializeFile,
) -> anyhow::Result<()> {
    let Some(update_flag) = update_flag(file.file_type) else {
        return Ok(());
    };
    if !destination_paths.insert(file.destination.clone()) {
        bail!(
            "destination `{}` was requested more than once",
            file.destination
        );
    }
    destinations
        .entry(Key::new(file.source_path, file.hgid))
        .or_default()
        .push((update_flag, file.destination));
    Ok(())
}

/// Recover manifest flags from the VFS update mode used for a fetched node.
fn manifest_file_type(update_flag: UpdateFlag) -> FileType {
    match update_flag {
        UpdateFlag::Regular => FileType::Regular,
        UpdateFlag::Executable => FileType::Executable,
        UpdateFlag::Symlink => FileType::Symlink,
    }
}

/// Convert lintable manifest entries into their materialized VFS mode.
fn update_flag(file_type: FileType) -> Option<UpdateFlag> {
    match file_type {
        FileType::Regular => Some(UpdateFlag::Regular),
        FileType::Executable => Some(UpdateFlag::Executable),
        FileType::Symlink | FileType::GitSubmodule => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use manifest_tree::testutil::TestStore;
    use storemodel::InsertOpts;
    use storemodel::KeyStore;
    use tempfile::tempdir;
    use types::testutil::hgid;
    use types::testutil::repo_path;
    use types::testutil::repo_path_buf;

    use super::*;

    #[test]
    fn writes_files_and_skips_symlinks() -> anyhow::Result<()> {
        let store = Arc::new(TestStore::new());
        insert(&store, "src/a.py", "11", b"print('a')\n")?;
        insert(&store, "src/b.py", "12", b"print('b')\n")?;
        insert(&store, "src/link.py", "13", b"a.py")?;
        let temp = tempdir()?;

        let written = materialize_files(
            temp.path().to_owned(),
            &(store.clone() as Arc<dyn FileStore>),
            vec![
                file("src/a.py", "11", FileType::Regular, "0/src/a.py"),
                file("src/a.py", "11", FileType::Executable, "1/src/a.py"),
                file("src/b.py", "12", FileType::Executable, "1/src/b.py"),
                file("src/link.py", "13", FileType::Symlink, "0/src/link.py"),
            ],
            WalkOptions::default(),
        )?;

        assert_eq!(
            written.keys().cloned().collect::<HashSet<_>>(),
            HashSet::from([
                repo_path_buf("0/src/a.py"),
                repo_path_buf("1/src/a.py"),
                repo_path_buf("1/src/b.py"),
            ])
        );
        assert_eq!(
            written[&repo_path_buf("0/src/a.py")],
            written[&repo_path_buf("1/src/a.py")]
        );
        assert_ne!(
            written[&repo_path_buf("0/src/a.py")],
            written[&repo_path_buf("1/src/b.py")]
        );
        assert_eq!(fs::read(temp.path().join("0/src/a.py"))?, b"print('a')\n");
        assert_eq!(fs::read(temp.path().join("1/src/a.py"))?, b"print('a')\n");
        assert_eq!(fs::read(temp.path().join("1/src/b.py"))?, b"print('b')\n");
        assert!(!temp.path().join("0/src/link.py").exists());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(temp.path().join("0/src/a.py"))?
                .permissions()
                .mode()
                & 0o111,
            0
        );
        #[cfg(unix)]
        assert_ne!(
            fs::metadata(temp.path().join("1/src/a.py"))?
                .permissions()
                .mode()
                & 0o111,
            0
        );
        assert_eq!(store.key_fetch_count(), 2);
        Ok(())
    }

    #[test]
    fn compares_linted_files_and_records_clean_outputs() -> anyhow::Result<()> {
        let temp = tempdir()?;
        fs::write(temp.path().join("same.py"), b"same\n")?;
        fs::write(temp.path().join("changed.py"), b"changed\n")?;
        fs::write(temp.path().join("large.py"), b"too large\n")?;
        let cache_dir = tempdir()?;
        let mut cache = LintCache::open(
            cache_dir.path(),
            &std::collections::BTreeMap::<&str, &str>::new(),
        )?;

        let result = compare_files(
            temp.path().to_owned(),
            vec![
                compare_file("src/same.py", "same.py", b"same\n"),
                compare_file("src/changed.py", "changed.py", b"original\n"),
                compare_file("src/large.py", "large.py", b"small\n"),
            ],
            Some(&mut cache),
            b"epoch",
            9,
        )?;

        assert_eq!(
            result.changed_files,
            vec![(
                repo_path_buf("changed.py"),
                ContentFingerprint::from_blob(&Blob::from_static(b"changed\n")),
            )]
        );
        assert_eq!(result.oversized_files, vec![repo_path_buf("large.py")]);
        // Both compared outputs are lint clean and get recorded under
        // their source path and output content; the oversized file does not.
        assert!(cache.contains(&LintCache::key(
            repo_path("src/same.py"),
            &Blob::from_static(b"same\n").blake3(),
            b"epoch",
        ))?);
        assert!(cache.contains(&LintCache::key(
            repo_path("src/changed.py"),
            &Blob::from_static(b"changed\n").blake3(),
            b"epoch",
        ))?);
        assert!(!cache.contains(&LintCache::key(
            repo_path("src/changed.py"),
            &Blob::from_static(b"original\n").blake3(),
            b"epoch",
        ))?);
        assert!(!cache.contains(&LintCache::key(
            repo_path("src/large.py"),
            &Blob::from_static(b"too large\n").blake3(),
            b"epoch",
        ))?);
        Ok(())
    }

    fn compare_file(source_path: &str, destination: &str, original: &[u8]) -> CompareFile {
        CompareFile {
            source_path: repo_path_buf(source_path),
            destination: repo_path_buf(destination),
            fingerprint: ContentFingerprint::from_blob(&Blob::from(original.to_vec())),
        }
    }

    #[test]
    fn prefilters_oversized_files_before_content_fetch() -> anyhow::Result<()> {
        let store = Arc::new(TestStore::new());
        insert(&store, "small.py", "41", b"ok\n")?;
        insert(&store, "large.py", "42", b"very large content\n")?;
        let small = Key::new(repo_path_buf("small.py"), hgid("41"));
        let large = Key::new(repo_path_buf("large.py"), hgid("42"));

        let result = prefilter_files(
            &(store as Arc<dyn FileStore>),
            vec![small.clone(), large],
            None,
            b"epoch",
            10,
        )?;

        assert_eq!(result.keep, vec![small], "only the small file should pass");
        assert_eq!(result.oversized_files, 1);
        assert_eq!(result.clean_files, 0);
        Ok(())
    }

    #[test]
    fn prefilters_versions_recorded_as_lint_clean() -> anyhow::Result<()> {
        let store = Arc::new(TestStore::new());
        insert(&store, "clean.py", "51", b"clean\n")?;
        insert(&store, "new.py", "52", b"new\n")?;
        let store = store as Arc<dyn FileStore>;
        let clean = Key::new(repo_path_buf("clean.py"), hgid("51"));
        let new = Key::new(repo_path_buf("new.py"), hgid("52"));

        let cache_dir = tempdir()?;
        let mut cache = LintCache::open(
            cache_dir.path(),
            &std::collections::BTreeMap::<&str, &str>::new(),
        )?;
        // Record from locally hashed content; the lookup below goes through aux
        // data, so this also asserts both derive the same keyed blake3.
        cache.record([LintCache::key(
            repo_path("clean.py"),
            &Blob::from_static(b"clean\n").blake3(),
            b"epoch",
        )])?;

        let result = prefilter_files(
            &store,
            vec![clean.clone(), new.clone()],
            Some(&cache),
            b"epoch",
            1024,
        )?;
        assert_eq!(result.keep, vec![new.clone()]);
        assert_eq!(result.clean_files, 1);

        let result = prefilter_files(
            &store,
            vec![clean.clone(), new.clone()],
            Some(&cache),
            b"other-epoch",
            1024,
        )?;
        assert_eq!(
            result.keep,
            vec![clean, new],
            "an epoch change should invalidate recorded entries"
        );
        assert_eq!(result.clean_files, 0);
        Ok(())
    }

    #[test]
    fn propagates_unavailable_content_errors() -> anyhow::Result<()> {
        let store = Arc::new(TestStore::new());
        insert(&store, "good.py", "31", b"good\n")?;
        let temp = tempdir()?;

        let result = materialize_files(
            temp.path().to_owned(),
            &(store as Arc<dyn FileStore>),
            vec![
                file("missing.py", "32", FileType::Regular, "0/missing.py"),
                file("good.py", "31", FileType::Regular, "0/good.py"),
            ],
            WalkOptions::default(),
        );

        let Err(error) = result else {
            bail!("missing content unexpectedly materialized")
        };
        assert!(format!("{error:#}").contains("missing.py"));
        assert_eq!(fs::read(temp.path().join("0/good.py"))?, b"good\n");
        Ok(())
    }

    fn file(
        source_path: &str,
        id: &str,
        file_type: FileType,
        destination: &str,
    ) -> MaterializeFile {
        MaterializeFile {
            source_path: repo_path_buf(source_path),
            hgid: hgid(id),
            file_type,
            destination: repo_path_buf(destination),
        }
    }

    fn insert(store: &TestStore, path: &str, id: &str, data: &[u8]) -> anyhow::Result<()> {
        store.insert_data(
            InsertOpts {
                forced_id: Some(Box::new(hgid(id))),
                ..Default::default()
            },
            repo_path(path),
            Blob::from(data.to_vec()),
        )?;
        Ok(())
    }
}
