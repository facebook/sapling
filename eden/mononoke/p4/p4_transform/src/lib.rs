/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Turns what Perforce reports about a changelist into Bonsai. The caller does all the
//! I/O, so the same inputs always give the same result.

use mononoke_types::ContentMetadataV2;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::NonRootMPath;
use p4_types::Action;
use p4_types::FileRevision;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransformError {
    #[error("depot root {0:?} is not like //depot/")]
    InvalidDepotRoot(String),
    #[error("{depot_path} is not a valid path in the repo")]
    InvalidPath {
        depot_path: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("no stored content for {0}")]
    MissingContent(String),
}

/// The change a file revision makes in the depot at `depot_root` (like `//test1/`), or
/// `None` if the file is in another depot. `content` is the revision's content, already in
/// the filestore, and is required for actions that leave content behind.
pub fn file_change(
    depot_root: &str,
    revision: &FileRevision,
    content: Option<&ContentMetadataV2>,
) -> Result<Option<(NonRootMPath, FileChange)>, TransformError> {
    // Exactly one depot: //test1 would also take //test10's files, and // every depot's.
    let depot_name = depot_root
        .strip_prefix("//")
        .and_then(|rest| rest.strip_suffix('/'));
    if !depot_name.is_some_and(|name| !name.is_empty() && !name.contains('/')) {
        return Err(TransformError::InvalidDepotRoot(depot_root.to_owned()));
    }
    let Some(path) = revision.depot_path.strip_prefix(depot_root) else {
        return Ok(None);
    };
    let path =
        NonRootMPath::new(unescape_path(path)).map_err(|source| TransformError::InvalidPath {
            depot_path: revision.depot_path.clone(),
            source,
        })?;
    let change = match revision.action {
        // Branch and MoveAdd are copies, but carry no copy_from until the caller can
        // resolve their source from filelog.
        Action::Add
        | Action::Edit
        | Action::Import
        | Action::Integrate
        | Action::Branch
        | Action::MoveAdd => {
            let content = content
                .ok_or_else(|| TransformError::MissingContent(revision.depot_path.clone()))?;
            FileChange::tracked(
                content.content_id,
                file_type(&revision.file_type),
                content.total_size,
                None,
                GitLfs::FullContent,
            )
        }
        // Purged and archived revisions have no content left to print.
        Action::Delete | Action::MoveDelete | Action::Purge | Action::Archive => {
            FileChange::Deletion
        }
    };
    Ok(Some((path, change)))
}

/// The executable ones among the old type names `p4 help filetypes` lists, like `xtext`
/// for `text+x`.
const LEGACY_EXECUTABLE_TYPES: [&str; 10] = [
    "cxtext", "kxtext", "uxbinary", "xbinary", "xltext", "xtempobj", "xtext", "xunicode", "xutf16",
    "xutf8",
];

/// A Perforce type is a base type and modifiers, like `text+kx`. Only `x` makes it
/// executable (`X` is the archive modifier).
fn file_type(p4_type: &str) -> FileType {
    let (base, modifiers) = p4_type.split_once('+').unwrap_or((p4_type, ""));
    if base == "symlink" {
        FileType::Symlink
    } else if modifiers.contains('x') || LEGACY_EXECUTABLE_TYPES.contains(&base) {
        FileType::Executable
    } else {
        FileType::Regular
    }
}

/// The characters Perforce stores escaped in depot paths, as `%` and two hex digits,
/// because they are revision and wildcard syntax. `%` is among them, so unescaping is
/// unambiguous.
const ESCAPES: [(char, &str); 4] = [('%', "25"), ('@', "40"), ('#', "23"), ('*', "2A")];

/// The real filename for a depot path as Perforce reports it, as `p4 sync` writes it to
/// disk: `foo%402x.png` is `foo@2x.png`. Repo paths are unescaped.
pub fn unescape_path(depot_path: &str) -> String {
    let mut segments = depot_path.split('%');
    let first = segments.next().unwrap_or_default().to_owned();
    segments.fold(first, |mut unescaped, segment| {
        match ESCAPES.iter().find(|(_, hex)| segment.starts_with(hex)) {
            Some((character, hex)) => {
                unescaped.push(*character);
                unescaped.push_str(&segment[hex.len()..]);
            }
            None => {
                unescaped.push('%');
                unescaped.push_str(segment);
            }
        }
        unescaped
    })
}

/// The depot path Perforce expects for a real filename, the inverse of `unescape_path`.
pub fn escape_path(path: &str) -> String {
    path.chars()
        .fold(String::with_capacity(path.len()), |mut escaped, c| {
            match ESCAPES.iter().find(|(character, _)| *character == c) {
                Some((_, hex)) => {
                    escaped.push('%');
                    escaped.push_str(hex);
                }
                None => escaped.push(c),
            }
            escaped
        })
}

#[cfg(test)]
mod tests;
