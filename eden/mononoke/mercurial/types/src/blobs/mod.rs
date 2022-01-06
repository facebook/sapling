/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod envelope;
pub use envelope::HgBlobEnvelope;

mod errors;
pub use errors::ErrorKind;

pub mod file;
pub use file::{File, LFSContent, META_MARKER, META_SZ};

mod manifest;
pub use self::manifest::{
    fetch_manifest_envelope, fetch_manifest_envelope_opt, fetch_raw_manifest_bytes, HgBlobManifest,
    ManifestContent,
};

mod changeset;
pub use changeset::{
    serialize_cs, serialize_extras, ChangesetMetadata, Extra, HgBlobChangeset, HgChangesetContent,
    RevlogChangeset,
};

pub mod filenode_lookup;

mod upload;
pub use upload::{
    ContentBlobMeta, UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash, UploadHgTreeEntry,
};
