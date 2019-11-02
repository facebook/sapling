/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs::Permissions, io::ErrorKind, path::PathBuf};

use failure::Fallible;
use tempfile::NamedTempFile;

use crate::error::EmptyMutablePack;

/// Mark the permission as read-only for user-group-other.
#[cfg(not(unix))]
fn make_readonly(perms: &mut Permissions) {
    perms.set_readonly(true);
}

#[cfg(unix)]
fn make_readonly(perms: &mut Permissions) {
    perms.set_mode(0o444);
}

/// Persist the temporary file.
///
/// Since packfiles are named based on their content, a rename failure due to an already existing
/// file isn't an error, as both files have effectively the same content.
fn persist(file: NamedTempFile, path: PathBuf) -> Fallible<()> {
    match file.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(e) => {
            if e.error.kind() != ErrorKind::AlreadyExists {
                Err(e.into())
            } else {
                Ok(())
            }
        }
    }
}

pub trait MutablePack {
    /// Make the data and index pack files with the data added to it. Also returns the fullpath of
    /// the files. After calling this function, the `MutablePack` is consumed and is no longer usable.
    fn build_files(self) -> Fallible<(NamedTempFile, NamedTempFile, PathBuf)>;

    /// Returns the extension for this kind of pack files.
    fn extension(&self) -> &'static str;

    /// Close the packfile, returning the path of the final immutable pack on disk. The
    /// `MutablePack` is no longer usable after being closed.
    fn close_pack(self) -> Fallible<Option<PathBuf>>
    where
        Self: Sized,
    {
        let extension = self.extension().to_string();
        let pack_extension = extension.clone() + "pack";
        let index_extension = extension + "idx";

        let (packfile, indexfile, base_filepath) = match self.build_files() {
            Err(err) => {
                if err.downcast_ref::<EmptyMutablePack>().is_some() {
                    return Ok(None);
                } else {
                    return Err(err);
                }
            }
            Ok(files) => files,
        };

        let mut perms = packfile.as_file().metadata()?.permissions();
        make_readonly(&mut perms);

        packfile.as_file().set_permissions(perms.clone())?;
        indexfile.as_file().set_permissions(perms)?;

        let packfile_path = base_filepath.with_extension(pack_extension);
        let indexfile_path = base_filepath.with_extension(index_extension);

        persist(packfile, packfile_path)?;
        persist(indexfile, indexfile_path)?;

        Ok(Some(base_filepath))
    }
}
