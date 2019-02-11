// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs::Permissions, path::PathBuf};

use failure::Fallible;
use tempfile::NamedTempFile;

/// Mark the permission as read-only for user-group-other.
#[cfg(not(unix))]
fn make_readonly(perms: &mut Permissions) {
    perms.set_readonly(true);
}

#[cfg(unix)]
fn make_readonly(perms: &mut Permissions) {
    perms.set_mode(0o444);
}

pub trait MutablePack {
    /// Make the data and index pack files with the data added to it. Also returns the fullpath of
    /// the files. After calling this function, the `MutablePack` is consumed and is no longer usable.
    fn build_files(self) -> Fallible<(NamedTempFile, NamedTempFile, PathBuf)>;

    /// Returns the extension for this kind of pack files.
    fn extension(&self) -> &'static str;

    /// Close the packfile, returning the path of the final immutable pack on disk. The
    /// `MutablePack` is no longer usable after being closed.
    fn close(self) -> Fallible<PathBuf>
    where
        Self: Sized,
    {
        let extension = self.extension().to_string();
        let pack_extension = extension.clone() + "pack";
        let index_extension = extension + "idx";

        let (packfile, indexfile, base_filepath) = self.build_files()?;

        let mut perms = packfile.as_file().metadata()?.permissions();
        make_readonly(&mut perms);

        packfile.as_file().set_permissions(perms.clone())?;
        indexfile.as_file().set_permissions(perms)?;

        packfile.persist(base_filepath.with_extension(pack_extension))?;
        indexfile.persist(base_filepath.with_extension(index_extension))?;

        Ok(base_filepath)
    }
}
