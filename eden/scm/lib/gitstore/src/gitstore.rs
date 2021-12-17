/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A simple store implementation to access a local git repo's odb.

use std::path::Path;

use types::HgId;

type Git2Result<T> = Result<T, git2::Error>;

pub struct GitStore {
    odb: git2::Odb<'static>,

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
    pub fn open(git_dir: &Path) -> Git2Result<Self> {
        let git_repo = git2::Repository::open(git_dir)?;
        let odb = git_repo.odb()?;

        struct UnsafeForceSync<T: ?Sized>(T);
        unsafe impl<T: ?Sized> Send for UnsafeForceSync<T> {}
        unsafe impl<T: ?Sized> Sync for UnsafeForceSync<T> {}
        impl Opaque for UnsafeForceSync<git2::Repository> {}

        // safety: `odb` is alive as long as `git_repo` is alive.
        let odb = unsafe { std::mem::transmute(odb) };
        // safety: we don't access `opaque_repo` in multiple threads.
        // Cast to `Opaque` and prevents access to `git_repo`.
        let opaque_repo: Box<dyn Opaque + Send + Sync> = Box::new(UnsafeForceSync(git_repo));

        let store = GitStore { odb, opaque_repo };
        Ok(store)
    }

    /// Read an object of the given type.
    pub fn read_obj(&self, id: HgId, kind: git2::ObjectType) -> Git2Result<Vec<u8>> {
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

    /// Write object to the odb.
    pub fn write_obj(&self, kind: git2::ObjectType, data: &[u8]) -> Git2Result<HgId> {
        let oid = self.odb.write(kind, data)?;
        let id = git_oid_to_hgid(oid);
        Ok(id)
    }
}

fn hgid_to_git_oid(id: HgId) -> git2::Oid {
    git2::Oid::from_bytes(id.as_ref()).expect("HgId should convert to git2::Oid")
}

fn git_oid_to_hgid(oid: git2::Oid) -> HgId {
    HgId::from_slice(oid.as_bytes()).expect("git2::Oid should convert to HgId")
}
