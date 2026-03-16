/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::fs::symlink_metadata;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use bitflags::bitflags;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use types::RepoPath;
use types::RepoPathBuf;

/// Audit repositories path to make sure that it is safe to write/remove through them.
///
/// This uses caching internally to avoid the heavy cost of querying the OS for each directory in
/// the path of a file.
///
/// The cache is concurrent and is shared between cloned instances of PathAuditor
pub struct PathAuditor {
    root: PathBuf,
    fs_features: FsFeatures,
    audited: DashMap<RepoPathBuf, ()>,
}

static WINDOWS_SHORTNAME_ALIASES: Lazy<WordSet> = Lazy::new(|| {
    let words = identity::sniff_idents()
        .map(|i| i.sniff_dot_dir.trim_start_matches('.'))
        .collect();
    WordSet::new(words)
});

static INVALID_COMPONENTS: Lazy<WordSet> = Lazy::new(|| {
    let components: [&'static str; 2] = [".", ".."];
    let words = components
        .into_iter()
        .chain(identity::sniff_idents().map(|i| i.sniff_dot_dir))
        .collect();
    WordSet::new(words)
});

/// A set of short words, for "contains" check.
struct WordSet {
    words_per_len: Vec<Vec<&'static str>>,
}

impl WordSet {
    fn new(words: Vec<&'static str>) -> Self {
        let max_len = words.iter().map(|w| w.len()).max().unwrap_or_default();
        let mut words_per_len: Vec<Vec<&'static str>> = Vec::with_capacity(max_len + 1);
        words_per_len.resize_with(max_len + 1, Default::default);
        for word in words {
            // Case-insensitive contains requires lowercase words.
            assert_eq!(word, word.to_lowercase());
            let words = &mut words_per_len[word.len()];
            words.push(word);
            // Case-insensitive contains uses u16 bits to track matches.
            assert!(words.len() < 16);
        }
        Self { words_per_len }
    }

    fn contains(&self, s: &str, case_insensitive: bool) -> bool {
        match self.words_per_len.get(s.len()) {
            Some(words) if !words.is_empty() => {
                if case_insensitive {
                    // Scan `s` byte-by-byte to avoid allocation.
                    let mut match_bits = u16::MAX;
                    for (byte_pos, b) in s.bytes().enumerate() {
                        let mut current_match_bits = 0u16;
                        let b = b.to_ascii_lowercase();
                        for (word_index, w) in words.iter().enumerate() {
                            if Some(&b) == w.as_bytes().get(byte_pos) {
                                current_match_bits |= 1u16 << word_index;
                            }
                        }
                        match_bits &= current_match_bits;
                        if match_bits == 0 {
                            return false;
                        }
                    }
                    match_bits != 0
                } else {
                    words.contains(&s)
                }
            }
            None | Some(_) => false,
        }
    }
}

bitflags! {
    #[derive(Copy, Clone)]
    pub struct FsFeatures: u32 {
        const CASE_INSENSITIVE = 1;
        /// `\` can be a path separator.
        const BACKSLASH_SEP = 2;
        /// Windows short names. e.g. "SL~1" might mean ".sl". Implies CASE_INSENSITIVE.
        const WINDOWS_NAMES = 5;
        /// Certain characters are ignored by HFS+.
        const HFS_STRIP = 8;
    }
}

impl FsFeatures {
    pub fn current_platform() -> Self {
        if cfg!(windows) {
            Self::BACKSLASH_SEP | Self::WINDOWS_NAMES
        } else if cfg!(target_os = "macos") {
            Self::HFS_STRIP
        } else {
            Self::empty()
        }
    }
}

// From encoding.py: These unicode characters are ignored by HFS+ (Apple Technote 1150,
// "Unicode Subtleties"), so we need to ignore them in some places for sanity.
const IGNORED_HFS_CHARS: [char; 16] = [
    '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}',
    '\u{202e}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}', '\u{206f}', '\u{feff}',
];

#[derive(thiserror::Error, Debug)]
pub enum AuditError {
    #[error("path '{0}' traverses symbolic link '{1}'")]
    ThroughSymlink(RepoPathBuf, RepoPathBuf),
    #[error("path contains illegal component '{0}': {1}")]
    InvalidComponent(String, String),
}

impl PathAuditor {
    pub fn new(root: impl AsRef<Path>, case_sensitive: bool) -> Self {
        let mut fs_features = FsFeatures::current_platform();
        if !case_sensitive {
            fs_features |= FsFeatures::CASE_INSENSITIVE;
        }
        let audited = Default::default();
        let root = root.as_ref().to_owned();
        Self {
            root,
            fs_features,
            audited,
        }
    }

    /// Slow path, query the filesystem for unsupported path. Namely, writing through a symlink
    /// outside of the repo is not supported.
    /// XXX: more checks
    fn audit_fs(&self, path: &RepoPath, orig_path: &RepoPath) -> Result<(), AuditError> {
        // Do not audit the vfs root.
        if path.is_empty() {
            return Ok(());
        }

        let full_path = self.root.join(path.as_str());

        // XXX: Maybe filter by specific errors?
        if let Ok(metadata) = symlink_metadata(full_path) {
            if metadata.file_type().is_symlink() {
                return Err(AuditError::ThroughSymlink(
                    orig_path.to_owned(),
                    path.to_owned(),
                ));
            }
        }

        Ok(())
    }

    /// Make sure that it is safe to write/remove `path` from the repo.
    pub fn audit(&self, path: &RepoPath) -> Result<PathBuf> {
        audit_invalid_components(path.as_str(), self.fs_features)?;

        let mut needs_recording_index = usize::MAX;
        for (i, parent) in path.reverse_parents().enumerate() {
            // First fast check w/ read lock
            if !self.audited.contains_key(parent) {
                // If fast check failed, do the stat syscall.
                self.audit_fs(parent, path)?;

                // If it passes the audit, we can't record them as audited just yet, since a parent
                // may still fail the audit. Later we'll loop through and record successful audits.
                needs_recording_index = i;
            } else {
                // path.parents() yields the results in deepest-first order, so if we hit a path
                // that has been audited, we know all the future ones have been audited and we can
                // bail early.
                break;
            }
        }

        if needs_recording_index != usize::MAX {
            for (i, parent) in path.reverse_parents().enumerate() {
                self.audited.entry(parent.to_owned()).or_default();
                if needs_recording_index == i {
                    break;
                }
            }
        }

        let mut filepath = self.root.to_owned();
        filepath.push(path.as_str());
        Ok(filepath)
    }
}

/// Checks that shortnames (e.g. `SL~1`) are not a component on Windows and that files don't end in
/// a dot (e.g. `sigh....`)
fn valid_windows_component(component: &str, fs_features: FsFeatures) -> bool {
    if !fs_features.contains(FsFeatures::WINDOWS_NAMES) {
        return true;
    }
    if let Some((l, r)) = component.split_once('~') {
        if r.chars().any(|c| c.is_numeric()) && WINDOWS_SHORTNAME_ALIASES.contains(l, true) {
            return false;
        }
    }
    !component.ends_with('.')
}

/// Makes sure that the path does not contain any of the following components:
/// - ``, empty, implies that paths can't start with, end or contain consecutive `SEPARATOR`s
/// - `.`, dot/period, unix current directory
/// - `..`, double dot, unix parent directory
/// - `.sl` or `.hg`,
///
/// It also checks that no trailing dots are part of the component and checks that shortnames
/// on Windows are valid.
pub fn audit_invalid_components(path: &str, fs_features: FsFeatures) -> Result<(), AuditError> {
    let separators: &[char] = if fs_features.contains(FsFeatures::BACKSLASH_SEP) {
        &['/', '\\'][..]
    } else {
        &['/'][..]
    };

    for s in path.split(separators) {
        if is_path_component_invalid(s, fs_features) {
            return Err(AuditError::InvalidComponent(s.to_owned(), path.to_owned()));
        }
    }
    Ok(())
}

/// Check if a path component is invalid. Returns `true` if invalid.
pub fn is_path_component_invalid(component: &str, fs_features: FsFeatures) -> bool {
    let s = component;
    let s = if fs_features.contains(FsFeatures::HFS_STRIP) && s.contains(IGNORED_HFS_CHARS) {
        Cow::Owned(s.replace(IGNORED_HFS_CHARS, ""))
    } else {
        Cow::Borrowed(s)
    };
    let case_insensitive = fs_features.contains(FsFeatures::CASE_INSENSITIVE);
    s.is_empty()
        || INVALID_COMPONENTS.contains(&s, case_insensitive)
        || !valid_windows_component(&s, fs_features)
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_audit_valid() -> Result<()> {
        let root = TempDir::new()?;

        let auditor = PathAuditor::new(&root, true);

        let repo_path = RepoPath::from_str("a/b")?;
        assert_eq!(
            auditor.audit(repo_path)?,
            root.as_ref().join(repo_path.as_str())
        );

        Ok(())
    }

    #[test]
    fn test_audit_invalid_components() -> Result<()> {
        let f = FsFeatures::empty();
        assert!(audit_invalid_components("a/../b", f).is_err());
        assert!(audit_invalid_components("a/./b", f).is_err());
        assert!(audit_invalid_components("a/.sl/b", f).is_err());
        assert!(audit_invalid_components("a/.hg/b", f).is_err());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn test_audit_windows() -> Result<()> {
        let root = TempDir::new()?;

        let auditor = PathAuditor::new(&root, true);

        let repo_path = RepoPath::from_str("..\\foobar")?;
        assert!(auditor.audit(repo_path).is_err());
        let repo_path = RepoPath::from_str("x/y/SL~123/z")?;
        assert!(auditor.audit(repo_path).is_err());
        let repo_path = RepoPath::from_str("sl~12345/baz")?;
        assert!(auditor.audit(repo_path).is_err());
        let repo_path = RepoPath::from_str("a/.sL")?;
        assert!(auditor.audit(repo_path).is_err());
        let repo_path = RepoPath::from_str("Sure...")?;
        assert!(auditor.audit(repo_path).is_err());

        Ok(())
    }

    #[cfg(not(windows))]
    #[test]
    fn test_audit_invalid_symlink() -> Result<()> {
        let root = TempDir::new()?;
        let other = TempDir::new()?;

        let auditor = PathAuditor::new(&root, true);

        let link = root.as_ref().join("a");
        std::os::unix::fs::symlink(&other, &link)?;
        let canonical_other = other.as_ref().canonicalize()?;
        assert_eq!(fs::read_link(&link)?.canonicalize()?, canonical_other);

        let repo_path = RepoPath::from_str("a/b")?;
        assert!(auditor.audit(repo_path).is_err());

        Ok(())
    }

    #[cfg(not(windows))]
    #[test]
    fn test_audit_caching() -> Result<()> {
        let root = TempDir::new()?;
        let other = TempDir::new()?;

        let path = root.as_ref().join("a");
        fs::create_dir_all(&path)?;

        let auditor = PathAuditor::new(&root, true);

        // Populate the auditor cache.
        let repo_path = RepoPath::from_str("a/b")?;
        auditor.audit(repo_path)?;

        fs::remove_dir_all(&path)?;

        let link = root.as_ref().join("a");
        std::os::unix::fs::symlink(&other, &link)?;
        let canonical_other = other.as_ref().canonicalize()?;
        assert_eq!(fs::read_link(&link)?.canonicalize()?, canonical_other);

        // Even though "a" is now a symlink to outside the repo, the audit will succeed due to the
        // one performed just above.
        let repo_path = RepoPath::from_str("a/b")?;
        auditor.audit(repo_path)?;

        Ok(())
    }

    #[test]
    fn test_valid_paths() {
        let all = FsFeatures::all();
        let none = FsFeatures::empty();
        assert!(audit_invalid_components("a/b/c", none).is_ok());
        assert!(audit_invalid_components("a/b/c", all).is_ok());
        assert!(audit_invalid_components("foo.bar/baz", none).is_ok());
        assert!(audit_invalid_components("foo.bar/baz", all).is_ok());
        assert!(audit_invalid_components("deeply/nested/path/to/file.txt", none).is_ok());
    }

    #[test]
    fn test_empty_components() {
        let f = FsFeatures::empty();
        assert!(audit_invalid_components("", f).is_err(), "empty path");
        assert!(audit_invalid_components("/a", f).is_err(), "leading /");
        assert!(audit_invalid_components("a/", f).is_err(), "trailing /");
        assert!(
            audit_invalid_components("a//b", f).is_err(),
            "consecutive /"
        );
    }

    #[test]
    fn test_hfs_invisible_chars() {
        let hfs = FsFeatures::HFS_STRIP;

        // HFS chars hiding dot-dirs
        assert!(audit_invalid_components("a/.\u{200c}sl/b", hfs).is_err());
        assert!(audit_invalid_components("a/.\u{200d}hg/b", hfs).is_err());
        assert!(audit_invalid_components("a/.\u{200c}\u{200d}sl/b", hfs).is_err());

        // HFS chars hiding "." and ".."
        assert!(audit_invalid_components("a/.\u{feff}/b", hfs).is_err());
        assert!(audit_invalid_components("a/.\u{feff}./b", hfs).is_err());

        // Component that reduces to empty after stripping
        assert!(audit_invalid_components("a/\u{200c}/b", hfs).is_err());

        // HFS chars in a normal component are fine
        assert!(audit_invalid_components("a/foo\u{200c}bar/b", hfs).is_ok());
    }

    #[test]
    fn test_windows_backslash_separator() {
        let bs = FsFeatures::BACKSLASH_SEP;

        // Backslash splits components
        assert!(audit_invalid_components("a\\.sl\\b", bs).is_err());
        assert!(audit_invalid_components("a\\..\\b", bs).is_err());
        assert!(audit_invalid_components("a\\.\\b", bs).is_err());

        // Mixed separators
        assert!(audit_invalid_components("a/.hg\\b", bs).is_err());

        // Without BACKSLASH_SEP, backslash is literal (part of component name)
        assert!(audit_invalid_components("a\\b", FsFeatures::empty()).is_ok());
    }

    #[test]
    fn test_windows_case_insensitive() {
        let ci = FsFeatures::CASE_INSENSITIVE;

        // Case-insensitive dot-dir matching
        assert!(audit_invalid_components("a/.SL/b", ci).is_err());
        assert!(audit_invalid_components("a/.Hg/b", ci).is_err());
        assert!(audit_invalid_components("a/.HG/b", ci).is_err());
        assert!(audit_invalid_components("a/.Sl/b", ci).is_err());

        // Without CASE_INSENSITIVE, uppercase variants are fine
        assert!(audit_invalid_components("a/.SL/b", FsFeatures::empty()).is_ok());
        assert!(audit_invalid_components("a/.HG/b", FsFeatures::empty()).is_ok());
    }

    #[test]
    fn test_windows_shortnames() {
        let wn = FsFeatures::WINDOWS_NAMES;

        assert!(audit_invalid_components("sl~1", wn).is_err());
        assert!(audit_invalid_components("hg~123", wn).is_err());

        assert!(audit_invalid_components("SL~1", wn).is_err());

        // No digits after ~ is fine
        assert!(audit_invalid_components("sl~abc", wn).is_ok());
        // Unknown prefix is fine
        assert!(audit_invalid_components("foo~1", wn).is_ok());
    }

    #[test]
    fn test_windows_trailing_dots() {
        let wn = FsFeatures::WINDOWS_NAMES;

        assert!(audit_invalid_components("file...", wn).is_err());
        assert!(audit_invalid_components("dir./foo", wn).is_err());

        // Without WINDOWS_NAMES, trailing dots are fine
        assert!(audit_invalid_components("file...", FsFeatures::empty()).is_ok());
        assert!(audit_invalid_components("dir./foo", FsFeatures::empty()).is_ok());
    }

    #[test]
    fn test_hfs_plus_case_insensitive_combined() {
        let f = FsFeatures::HFS_STRIP | FsFeatures::CASE_INSENSITIVE;

        // HFS invisible char + case insensitivity
        assert!(audit_invalid_components("a/.\u{200c}SL/b", f).is_err());
        assert!(audit_invalid_components("a/.\u{feff}Hg/b", f).is_err());
    }

    #[test]
    fn test_all_identity_dotdir() {
        let f = FsFeatures::empty();
        assert!(audit_invalid_components("a/.git/b", f).is_err());

        let f = FsFeatures::WINDOWS_NAMES;
        assert!(audit_invalid_components("a/GiT~1/b", f).is_err());
    }
}
