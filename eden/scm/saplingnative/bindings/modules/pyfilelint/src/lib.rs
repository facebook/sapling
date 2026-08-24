/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::path::PathBuf;

use ::serde::Serialize;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use cpython::*;
use cpython_ext::ResultPyErrExt;
use cpython_ext::convert::Serde;
use filelint::CompareFile;
use filelint::ContentFingerprint;
use filelint::LintCache;
use filelint::MaterializeFile;
use filelint::compare_files;
use filelint::materialize_files;
use filelint::prefilter_files;
use filewalk::WalkOptions;
use manifest::FileType;
use pyrepo::repo as PyRepo;
use revisionstore::util::get_cache_path;
use revisionstore::util::peek_cache_path;
use types::Blake3;
use types::HgId;
use types::Key;
use types::RepoPathBuf;

#[expect(
    clippy::manual_strip,
    reason = "false positive in cpython's py_fn macro"
)]
pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "filelint"].join(".");
    let module = PyModule::new(py, &name)?;
    module.add_class::<lintstate>(py)?;
    module.add(py, "clearcache", py_fn!(py, clear_cache_py(repo: PyRepo)))?;
    Ok(module)
}

// Per-run lint state: native calls share one repo handle and one lint
// clean-content cache, opened on first use instead of once per call.
py_class!(pub class lintstate |py| {
    data repo: PyRepo;
    // `None` until the first cache user; then the `open_cache` result.
    data cache: RefCell<Option<Option<LintCache>>>;

    def __new__(_cls, repo: PyRepo) -> PyResult<lintstate> {
        lintstate::create_instance(py, repo, RefCell::new(None))
    }

    /// Drop oversized and already lint-clean file versions using batched aux data.
    ///
    /// prefilter(files: [(path, node)])
    ///   -> {"files": [(path, node)], "oversized_files": int, "clean_files": int}
    def prefilter(
        &self,
        files: Serde<Vec<(RepoPathBuf, HgId)>>
    ) -> PyResult<Serde<PrefilterOutput>> {
        let keys = files
            .0
            .into_iter()
            .map(|(path, hgid)| Key::new(path, hgid))
            .collect();
        let (file_store, config) = {
            let repo = self.repo(py).read_repo(py);
            (repo.file_store().map_pyerr(py)?, repo.config().clone())
        };
        let max_file_size = max_file_size(config.as_ref()).map_pyerr(py)?;
        let cache = self.take_cache(py, config.as_ref());
        let (result, cache) = py.allow_threads(move || {
            let epoch = cache_epoch(config.as_ref());
            let result = prefilter_files(&file_store, keys, cache.as_ref(), &epoch, max_file_size);
            (result, cache)
        });
        self.put_cache(py, cache);
        let result = result.map_pyerr(py)?;

        Ok(Serde(PrefilterOutput {
            files: result
                .keep
                .into_iter()
                .map(|key| (key.path, key.hgid))
                .collect(),
            oversized_files: result.oversized_files,
            clean_files: result.clean_files,
        }))
    }

    /// Fetch explicit file nodes and write them to their requested destinations.
    ///
    /// materialize(output_root, files: [(source_path, node, flags, destination)])
    ///   -> [(destination, size, blake3)]
    def materialize(
        &self,
        output_root: String,
        files: Serde<Vec<(RepoPathBuf, HgId, String, RepoPathBuf)>>
    ) -> PyResult<Serde<Vec<(RepoPathBuf, usize, Blake3)>>> {
        let mut requests = Vec::with_capacity(files.0.len());
        for (source_path, hgid, flags, destination) in files.0 {
            requests.push(MaterializeFile {
                source_path,
                hgid,
                file_type: file_type(py, &flags)?,
                destination,
            });
        }
        let (file_store, walk_options) = {
            let repo = self.repo(py).read_repo(py);
            (
                repo.file_store().map_pyerr(py)?,
                WalkOptions::from_config(repo.config().as_ref()).map_pyerr(py)?,
            )
        };
        let written = py
            .allow_threads(move || {
                materialize_files(
                    PathBuf::from(output_root),
                    &file_store,
                    requests,
                    walk_options,
                )
            })
            .map_pyerr(py)?;

        Ok(Serde(
            written
                .into_iter()
                .map(|(path, fingerprint)| (path, fingerprint.size, fingerprint.blake3))
                .collect(),
        ))
    }

    /// Compare staged outputs with their original fingerprints and return
    /// changed destinations, recording lint-clean content in the cache.
    ///
    /// compare(output_root, files: [(source_path, destination, size, blake3)], record)
    ///   -> {"changed_files": [(destination, size, blake3)], "oversized_files": [destination]}
    def compare(
        &self,
        output_root: String,
        files: Serde<Vec<(RepoPathBuf, RepoPathBuf, usize, Blake3)>>,
        record: bool
    ) -> PyResult<Serde<CompareOutput>> {
        let files = files
            .0
            .into_iter()
            .map(|(source_path, destination, size, blake3)| CompareFile {
                source_path,
                destination,
                fingerprint: ContentFingerprint { size, blake3 },
            })
            .collect();
        let config = self.repo(py).read_repo(py).config().clone();
        let max_file_size = max_file_size(config.as_ref()).map_pyerr(py)?;
        // Recording marks content as clean for every configured linter,
        // so callers skip it for comparisons taken mid linter sequence.
        let mut cache = if record {
            self.take_cache(py, config.as_ref())
        } else {
            None
        };
        let (result, cache) = py.allow_threads(move || {
            let epoch = cache_epoch(config.as_ref());
            let result = compare_files(
                PathBuf::from(output_root),
                files,
                cache.as_mut(),
                &epoch,
                max_file_size,
            );
            (result, cache)
        });
        if record {
            self.put_cache(py, cache);
        }
        let result = result.map_pyerr(py)?;

        Ok(Serde(CompareOutput {
            changed_files: result
                .changed_files
                .into_iter()
                .map(|(path, fingerprint)| (path, fingerprint.size, fingerprint.blake3))
                .collect(),
            oversized_files: result.oversized_files,
        }))
    }
});

impl lintstate {
    /// Hand out the shared cache for one native call; `put_cache` returns
    /// it afterwards. The first user pays for the open (`None` on failure
    /// or when disabled, and that outcome is reused too).
    fn take_cache(&self, py: Python, config: &dyn Config) -> Option<LintCache> {
        self.cache(py)
            .borrow_mut()
            .take()
            .unwrap_or_else(|| open_cache(config))
    }

    fn put_cache(&self, py: Python, cache: Option<LintCache>) {
        *self.cache(py).borrow_mut() = Some(cache);
    }
}

#[derive(Serialize)]
struct PrefilterOutput {
    files: Vec<(RepoPathBuf, HgId)>,
    oversized_files: usize,
    clean_files: usize,
}

#[derive(Serialize)]
struct CompareOutput {
    changed_files: Vec<(RepoPathBuf, usize, Blake3)>,
    oversized_files: Vec<RepoPathBuf>,
}

/// Remove every recorded lint clean-content entry, best effort.
///
/// Returns an error message when clearing fails (ex. another process holds
/// the cache files open on Windows). The cache only skips work, so callers
/// warn instead of aborting.
fn clear_cache_py(py: Python, repo: PyRepo) -> PyResult<Option<String>> {
    let config = repo.read_repo(py).config().clone();
    let result = py.allow_threads(move || -> anyhow::Result<()> {
        // peek_cache_path avoids get_cache_path's mkdir side effect:
        // clearing should not create the directory it is about to remove.
        if let Some(mut dir) = peek_cache_path(config.as_ref())? {
            dir.push("filelint");
            LintCache::clear(&dir)?;
        }
        Ok(())
    });
    Ok(result.err().map(|err| format!("{err:#}")))
}

/// Size limit for lintable content. Always present via the builtin core config.
fn max_file_size(config: &dyn Config) -> anyhow::Result<usize> {
    Ok(config
        .get_or_default::<ByteCount>("filelint", "max-file-size")?
        .value() as usize)
}

/// Epoch mixed into every cache key. Bumping `filelint.cache-epoch`
/// invalidates all recorded entries, e.g. after a linter behavior change.
fn cache_epoch(config: &dyn Config) -> Vec<u8> {
    config
        .get("filelint", "cache-epoch")
        .map(|value| value.as_bytes().to_vec())
        .unwrap_or_default()
}

/// Open the lint clean-content cache in the shared repository cache.
///
/// Formatting works without the cache, so failures only disable it.
fn open_cache(config: &dyn Config) -> Option<LintCache> {
    match try_open_cache(config) {
        Ok(cache) => cache,
        Err(err) => {
            tracing::warn!(?err, "error opening lint clean-content cache");
            None
        }
    }
}

fn try_open_cache(config: &dyn Config) -> anyhow::Result<Option<LintCache>> {
    if !config.get_or("filelint", "cache", || true)? {
        return Ok(None);
    }
    let Some(dir) = get_cache_path(config, &Some("filelint"))? else {
        return Ok(None);
    };
    Ok(Some(LintCache::open(&dir, config)?))
}

fn file_type(py: Python, flags: &str) -> PyResult<FileType> {
    match flags {
        "" => Ok(FileType::Regular),
        "x" => Ok(FileType::Executable),
        // Symlinks and submodules cannot be linted; callers filter them
        // out, so treat them as errors rather than dropping them silently.
        _ => Err(PyErr::new::<exc::ValueError, _>(
            py,
            format!("cannot materialize file with flags `{flags}`"),
        )),
    }
}
