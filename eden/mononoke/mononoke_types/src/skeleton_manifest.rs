/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Result};

use crate::blob::{Blob, BlobstoreValue, SkeletonManifestBlob};
use crate::errors::ErrorKind;
use crate::path::{MPath, MPathElement};
use crate::thrift;
use crate::typed_hash::{SkeletonManifestId, SkeletonManifestIdContext};

use blobstore::{Blobstore, StoreLoadable};
use context::CoreContext;
use fbthrift::compact_protocol;
use sorted_vector_map::SortedVectorMap;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

/// A skeleton manifest is a manifest node containing summary information about the
/// the structure of files (their names, but not their contents) that is useful
/// for traversing manifests and for determining case conflicts and path name
/// compatibility.
///
/// Skeleton manifests only exist for trees, and each skeleton manifest is a structure that contains:
/// * A list of its children, containing for each child:
///   - Name
///   - Whether is is a directory or not
///   - The skeleton manifest id, summary flags counts for directories.
/// * The summary flags and counts for the directory itself.
///
/// The summary flags stored for each directory are:
/// * whether the directory's immediate children contain a case conflict.
/// * whether any descendant directories contain a case conflict.
/// * whether the directory's immediate children include a filename that is
///   invalid on Windows.
/// * whether the descendant directories contain a filename that is invalid on
///   Windows.
///
/// The summary counts stored for each directory are:
/// * recursive count of descendant sub-directories
/// * maximum path length in the directory
/// * maximum path element length for any child directory
/// * maximum path element length for the contents of descendant directories
///
/// Path element and path lengths are measured in bytes.
///
/// Unlike unodes, skeleton manifests are not repository-wide unique. Unlike
/// fsnodes, they are not content-addressed.  If the same set of file names and
/// directory names appear at different places in the commit graph, they will
/// share skeleton manifests.

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SkeletonManifest {
    subentries: SortedVectorMap<MPathElement, SkeletonManifestEntry>,
    summary: SkeletonManifestSummary,
}

impl SkeletonManifest {
    pub fn new(
        subentries: SortedVectorMap<MPathElement, SkeletonManifestEntry>,
        summary: SkeletonManifestSummary,
    ) -> Self {
        Self {
            subentries,
            summary,
        }
    }

    pub fn lookup(&self, basename: &MPathElement) -> Option<&SkeletonManifestEntry> {
        self.subentries.get(basename)
    }

    pub fn list(&self) -> impl Iterator<Item = (&MPathElement, &SkeletonManifestEntry)> {
        self.subentries.iter()
    }

    pub fn into_subentries(self) -> SortedVectorMap<MPathElement, SkeletonManifestEntry> {
        self.subentries
    }

    pub fn summary(&self) -> &SkeletonManifestSummary {
        &self.summary
    }

    pub async fn first_case_conflict(
        &self,
        ctx: &CoreContext,
        blobstore: &(impl Blobstore + Clone),
    ) -> Result<Option<(MPath, MPath)>> {
        let mut sk_mf = Cow::Borrowed(self);
        let mut path: Option<MPath> = None;
        'outer: loop {
            if sk_mf.summary.child_case_conflicts {
                let mut lower_map = HashMap::new();
                for name in sk_mf.subentries.keys() {
                    if let Some(lower_name) = name.to_lowercase_utf8() {
                        if let Some(other_name) = lower_map.insert(lower_name, name.clone()) {
                            return Ok(Some((
                                MPath::join_opt_element(path.as_ref(), &other_name),
                                MPath::join_opt_element(path.as_ref(), name),
                            )));
                        }
                    }
                }
            }
            if sk_mf.summary.descendant_case_conflicts {
                for (name, entry) in sk_mf.subentries.iter() {
                    if let SkeletonManifestEntry::Directory(subdir) = entry {
                        if subdir.summary.child_case_conflicts
                            || subdir.summary.descendant_case_conflicts
                        {
                            path = Some(MPath::join_opt_element(path.as_ref(), name));
                            sk_mf = Cow::Owned(subdir.id.load(ctx.clone(), blobstore).await?);
                            continue 'outer;
                        }
                    }
                }
            }
            return Ok(None);
        }
    }

    pub(crate) fn from_thrift(t: thrift::SkeletonManifest) -> Result<SkeletonManifest> {
        let subentries = t
            .subentries
            .into_iter()
            .map(|(basename, skeleton_entry)| {
                let basename = MPathElement::from_thrift(basename)?;
                let skeleton_entry = SkeletonManifestEntry::from_thrift(skeleton_entry)?;
                Ok((basename, skeleton_entry))
            })
            .collect::<Result<_>>()?;
        let summary = SkeletonManifestSummary::from_thrift(t.summary)?;
        Ok(SkeletonManifest {
            subentries,
            summary,
        })
    }

    pub(crate) fn into_thrift(self) -> thrift::SkeletonManifest {
        let subentries: BTreeMap<_, _> = self
            .subentries
            .into_iter()
            .map(|(basename, fsnode_entry)| (basename.into_thrift(), fsnode_entry.into_thrift()))
            .collect();
        let summary = self.summary.into_thrift();
        thrift::SkeletonManifest {
            subentries,
            summary,
        }
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(bytes)
            .with_context(|| ErrorKind::BlobDeserializeError("SkeletonManifest".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum SkeletonManifestEntry {
    File,
    Directory(SkeletonManifestDirectory),
}

impl SkeletonManifestEntry {
    pub(crate) fn from_thrift(t: thrift::SkeletonManifestEntry) -> Result<SkeletonManifestEntry> {
        match t.directory {
            None => Ok(SkeletonManifestEntry::File),
            Some(skeleton_directory) => {
                let skeleton_directory =
                    SkeletonManifestDirectory::from_thrift(skeleton_directory)?;
                Ok(SkeletonManifestEntry::Directory(skeleton_directory))
            }
        }
    }

    pub(crate) fn into_thrift(self) -> thrift::SkeletonManifestEntry {
        let directory = match self {
            SkeletonManifestEntry::File => None,
            SkeletonManifestEntry::Directory(skeleton_directory) => {
                Some(skeleton_directory.into_thrift())
            }
        };
        thrift::SkeletonManifestEntry { directory }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SkeletonManifestDirectory {
    id: SkeletonManifestId,
    summary: SkeletonManifestSummary,
}

impl SkeletonManifestDirectory {
    pub fn new(id: SkeletonManifestId, summary: SkeletonManifestSummary) -> Self {
        Self { id, summary }
    }

    pub fn id(&self) -> &SkeletonManifestId {
        &self.id
    }

    pub fn summary(&self) -> &SkeletonManifestSummary {
        &self.summary
    }

    pub(crate) fn from_thrift(
        t: thrift::SkeletonManifestDirectory,
    ) -> Result<SkeletonManifestDirectory> {
        let id = SkeletonManifestId::from_thrift(t.id)?;
        let summary = SkeletonManifestSummary::from_thrift(t.summary)?;
        Ok(SkeletonManifestDirectory { id, summary })
    }

    pub(crate) fn into_thrift(self) -> thrift::SkeletonManifestDirectory {
        thrift::SkeletonManifestDirectory {
            id: self.id.into_thrift(),
            summary: self.summary.into_thrift(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Default)]
pub struct SkeletonManifestSummary {
    pub child_files_count: u64,
    pub descendant_files_count: u64,
    pub child_dirs_count: u64,
    pub descendant_dirs_count: u64,
    pub max_path_len: u32,
    pub max_path_wchar_len: u32,
    pub child_case_conflicts: bool,
    pub descendant_case_conflicts: bool,
    pub child_non_utf8_filenames: bool,
    pub descendant_non_utf8_filenames: bool,
    pub child_invalid_windows_filenames: bool,
    pub descendant_invalid_windows_filenames: bool,
}

impl SkeletonManifestSummary {
    pub(crate) fn from_thrift(
        t: thrift::SkeletonManifestSummary,
    ) -> Result<SkeletonManifestSummary> {
        Ok(SkeletonManifestSummary {
            child_files_count: t.child_files_count as u64,
            descendant_files_count: t.descendant_files_count as u64,
            child_dirs_count: t.child_dirs_count as u64,
            descendant_dirs_count: t.descendant_dirs_count as u64,
            max_path_len: t.max_path_len as u32,
            max_path_wchar_len: t.max_path_wchar_len as u32,
            child_case_conflicts: t.child_case_conflicts,
            descendant_case_conflicts: t.descendant_case_conflicts,
            child_non_utf8_filenames: t.child_non_utf8_filenames,
            descendant_non_utf8_filenames: t.descendant_non_utf8_filenames,
            child_invalid_windows_filenames: t.child_invalid_windows_filenames,
            descendant_invalid_windows_filenames: t.descendant_invalid_windows_filenames,
        })
    }

    pub(crate) fn into_thrift(self) -> thrift::SkeletonManifestSummary {
        thrift::SkeletonManifestSummary {
            child_files_count: self.child_files_count as i64,
            descendant_files_count: self.descendant_files_count as i64,
            child_dirs_count: self.child_dirs_count as i64,
            descendant_dirs_count: self.descendant_dirs_count as i64,
            max_path_len: self.max_path_len as i32,
            max_path_wchar_len: self.max_path_wchar_len as i32,
            child_case_conflicts: self.child_case_conflicts,
            descendant_case_conflicts: self.descendant_case_conflicts,
            child_non_utf8_filenames: self.child_non_utf8_filenames,
            descendant_non_utf8_filenames: self.descendant_non_utf8_filenames,
            child_invalid_windows_filenames: self.child_invalid_windows_filenames,
            descendant_invalid_windows_filenames: self.descendant_invalid_windows_filenames,
        }
    }
}

impl BlobstoreValue for SkeletonManifest {
    type Key = SkeletonManifestId;

    fn into_blob(self) -> SkeletonManifestBlob {
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        let mut context = SkeletonManifestIdContext::new();
        context.update(&data);
        let id = context.finish();
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data().as_ref())
    }
}
