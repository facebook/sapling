/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A simple store implementation to access a local git repo's odb.

use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use configmodel::Config;
use gitcompat::rungit::RunGitOptions;
use types::HgId;

type Git2Result<T> = Result<T, git2::Error>;

pub struct GitStore {
    odb: git2::Odb<'static>,

    git: RunGitOptions,

    /// If set, fetch missing objects on demand from the URL.
    fetch_url: Option<String>,
    fetch_filter: String,

    // Makes `odb` valid. Last field drops last.
    // No need to use this field. Just need to keep it alive.
    // Use `Opaque` to forbid access to the underlying repo.
    // See also `safety` notes in `GitStore::open`.
    #[allow(dead_code)]
    opaque_repo: Box<dyn Opaque + Send + Sync>,
}

trait Opaque {}

impl GitStore {
    /// `open` a Git bare repo at `git_dir`. Gain access to its odb (object database).
    pub fn open(git_dir: &Path, config: &dyn Config) -> Git2Result<Self> {
        let git_repo = git2::Repository::open(git_dir)?;
        let odb = git_repo.odb()?;

        let mut git = RunGitOptions::from_config(config);
        git.git_dir = Some(git_repo.path().to_owned());

        // Git's negotiation algorithm works on commit reference level and can add significant
        // overhead if we simply want to fetch trees or blobs.
        // See also Git's promisor-remote which sets the same config:
        // https://github.com/git/git/blob/b3d1c85d4833aef546f11e4d37516a1ececaefc3/promisor-remote.c#L30
        git.extra_git_configs
            .push("fetch.negotiationAlgorithm=noop".to_string());

        let fetch_url = config.get("paths", "default").map(|s| s.to_string());

        // "filter" passed to `git fetch`. "blob:none" is used by Git's promisor-remote but that
        // does not deduplicate trees. "tree:0" more aggressively deduplicates trees but might
        // cause more network round trips.
        let fetch_filter = {
            let config = config.get("git", "filter");
            let config = match &config {
                Some(v) => v.as_ref(),
                // PERF: Ideally this is "tree:0" but the tree diff is currently sequential...
                None => "blob:none",
            };
            format!("--filter={}", config)
        };

        struct UnsafeForceSync<T: ?Sized>(T);
        unsafe impl<T: ?Sized> Send for UnsafeForceSync<T> {}
        unsafe impl<T: ?Sized> Sync for UnsafeForceSync<T> {}
        impl Opaque for UnsafeForceSync<git2::Repository> {}

        // safety: `odb` is alive as long as `git_repo` is alive.
        let odb = unsafe { std::mem::transmute(odb) };
        // safety: we don't access `opaque_repo` in multiple threads.
        // Cast to `Opaque` and prevents access to `git_repo`.
        let opaque_repo: Box<dyn Opaque + Send + Sync> = Box::new(UnsafeForceSync(git_repo));

        let store = GitStore {
            odb,
            git,
            fetch_url,
            fetch_filter,
            opaque_repo,
        };
        Ok(store)
    }

    /// Read an object of the given type.
    pub fn read_obj(&self, id: HgId, kind: git2::ObjectType) -> Git2Result<Vec<u8>> {
        if id.is_null() {
            return Ok(Vec::new());
        }
        let oid = hgid_to_git_oid(id);
        let obj = self.odb.read(oid)?;
        if kind != git2::ObjectType::Any && obj.kind() != kind {
            return Err(git2::Error::new(
                git2::ErrorCode::NotFound,
                git2::ErrorClass::Object,
                format!("{} {} not found", kind, oid),
            ));
        }
        Ok(obj.data().to_vec())
    }

    /// Read the size of an object without its full content.
    pub fn read_obj_size(&self, id: HgId, kind: git2::ObjectType) -> Git2Result<usize> {
        if id.is_null() {
            return Ok(0);
        }
        let oid = hgid_to_git_oid(id);
        let (size, obj_kind) = self.odb.read_header(oid)?;
        if kind != git2::ObjectType::Any && obj_kind != kind {
            return Err(git2::Error::new(
                git2::ErrorCode::NotFound,
                git2::ErrorClass::Object,
                format!("{} {} not found", kind, oid),
            ));
        }
        Ok(size)
    }

    /// Write object to the odb.
    pub fn write_obj(&self, kind: git2::ObjectType, data: &[u8]) -> Git2Result<HgId> {
        let oid = self.odb.write(kind, data)?;
        let id = git_oid_to_hgid(oid);
        Ok(id)
    }

    /// Fetch the oids from fetch_remote. Existing oids will be skipped.
    /// If every oid exists locally, then no `git fetch` process is spawned.
    /// Otherwise, block until the `git fetch` command completes.
    /// Currently, does not check the exit code of `git fetch`.
    pub fn fetch_objs(&self, ids: &[HgId]) -> anyhow::Result<()> {
        let mut missing_ids = ids.iter().filter(|id| {
            let id = hgid_to_git_oid(**id);
            // For performance, disable refresh here.
            !self.odb.exists_ext(id, git2::OdbLookupFlags::NO_REFRESH)
        });

        let first_missing_id = match missing_ids.next() {
            // No need to fetch.
            None => return Ok(()),
            Some(id) => id,
        };
        let missing_ids = std::iter::once(first_missing_id).chain(missing_ids);

        let url = match self.fetch_url.as_ref() {
            Some(url) => url,
            None => anyhow::bail!("paths.default is not set to fetch remotely"),
        };

        // See also git/promisor-remote.c
        let args = [
            url,
            "--no-tags",
            // TODO: Upgrade Git so it supports this flag.
            #[cfg(not(windows))]
            "--no-write-fetch-head",
            "--recurse-submodules=no",
            &self.fetch_filter,
            "--stdin",
        ];
        let mut child = self
            .git
            .git_cmd("fetch", &args)
            .stdin(Stdio::piped())
            .spawn()?;
        if let Some(stdin) = child.stdin.take() {
            let mut stdin = BufWriter::new(stdin);
            for id in missing_ids {
                let hex = id.to_hex();
                stdin.write_all(hex.as_bytes())?;
                stdin.write_all(b"\n")?;
            }
            drop(stdin);
        }

        child.wait()?;
        Ok(())
    }

    /// Returns true if `fetch_url` is set.
    pub fn has_fetch_url(&self) -> bool {
        self.fetch_url.is_some()
    }
}

fn hgid_to_git_oid(id: HgId) -> git2::Oid {
    git2::Oid::from_bytes(id.as_ref()).expect("HgId should convert to git2::Oid")
}

fn git_oid_to_hgid(oid: git2::Oid) -> HgId {
    HgId::from_slice(oid.as_bytes()).expect("git2::Oid should convert to HgId")
}
