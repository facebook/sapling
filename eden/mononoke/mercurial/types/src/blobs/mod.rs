/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

mod envelope;
pub use envelope::HgBlobEnvelope;

mod errors;
pub use errors::ErrorKind;

pub mod file;
pub use file::{File, HgBlobEntry, LFSContent, META_MARKER, META_SZ};

mod manifest;
pub use self::manifest::{
    fetch_manifest_envelope, fetch_raw_manifest_bytes, BlobManifest, ManifestContent,
};

mod changeset;
pub use changeset::{
    serialize_cs, serialize_extras, ChangesetMetadata, Extra, HgBlobChangeset, HgChangesetContent,
    RevlogChangeset,
};

pub mod filenode_lookup;

mod upload;
pub use upload::{
    ContentBlobInfo, ContentBlobMeta, UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash,
    UploadHgTreeEntry,
};
