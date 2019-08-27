// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

mod envelope;
pub use envelope::HgBlobEnvelope;

mod errors;
pub use errors::ErrorKind;

pub mod file;
pub use file::{
    fetch_file_content_from_blobstore, fetch_file_content_id_from_blobstore,
    fetch_file_content_sha256_from_blobstore, fetch_file_contents, fetch_file_envelope,
    fetch_file_metadata_from_blobstore, fetch_file_parents_from_blobstore,
    fetch_file_size_from_blobstore, File, HgBlobEntry, LFSContent, META_MARKER, META_SZ,
};

mod manifest;
pub use self::manifest::{BlobManifest, ManifestContent};
