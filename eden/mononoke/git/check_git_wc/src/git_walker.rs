/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use git2::ObjectType;
use git2::Oid;
use git2::Repository;
use git2::Tree;
use git2::TreeEntry;
use mononoke_types::hash;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types::RepoPath;
use std::collections::HashMap;
use std::str::FromStr;
use tokio::sync::mpsc;

use crate::CheckEntry;
use crate::CheckNode;

fn get_sha256(git_lfs: bool, contents: &[u8]) -> hash::Sha256 {
    if git_lfs
        && (contents.starts_with(b"version https://git-lfs.github.com/spec/v1\n")
            || contents.starts_with(b"version https://hawser.github.com/spec/v1\n"))
    {
        if let Ok(pointer) = std::str::from_utf8(contents) {
            // This is a plausible pointer - it's UTF-8, it begins with a version line
            // Skip the version, and look for the embedded hash
            for line in pointer.lines().skip(1) {
                if line > "oid sha256:g" {
                    // Lines apart from version must be in ASCII order. If we get past the last
                    // possible place for our OID, then we should break out.
                    // That way, a file that starts with a known version line but then has random
                    // junk is not going to consume our time
                    break;
                }
                if let Some(hash) = line.strip_prefix("oid sha256:") {
                    // Possible SHA256 for content. Extract if possible
                    if let Ok(hash) = hash::Sha256::from_str(hash) {
                        return hash;
                    } else {
                        // Not a valid SHA256, so not actually a pointer file
                        break;
                    }
                }
            }
        }
    }
    use sha2::Digest;
    use sha2::Sha256;
    let mut hasher = Sha256::new();
    hasher.update(contents);
    hash::Sha256::from_byte_array(hasher.finalize().into())
}

fn process_entry(git_lfs: bool, repo: &Repository, entry: TreeEntry<'_>) -> Result<CheckEntry> {
    let filemode = entry.filemode();

    match entry.kind() {
        Some(ObjectType::Blob) => {
            let obj = entry.to_object(repo)?;
            let blob = obj.peel_to_blob()?;
            let hash = get_sha256(git_lfs, blob.content());
            let filetype = match filemode {
                0o100644 => FileType::Regular,
                0o100755 => FileType::Executable,
                0o120000 => FileType::Symlink,
                _ => bail!("Unknown git filemode {}", filemode),
            };

            Ok(CheckEntry::File(filetype, hash))
        }
        Some(ObjectType::Tree) => Ok(CheckEntry::Directory),
        Some(ObjectType::Commit) => {
            // A commit in a tree is a submodule. Treat as empty directory - this is what
            // `git checkout` does when submodules aren't enabled
            Ok(CheckEntry::Directory)
        }
        kind => Err(anyhow!("Object is of unexpected kind {:?}", kind)),
    }
}

fn walk_tree(
    git_lfs: bool,
    path: RepoPath,
    tx: &mpsc::Sender<CheckNode>,
    repo: &Repository,
    tree: Tree<'_>,
) -> Result<()> {
    let contents: HashMap<MPathElement, CheckEntry> = tree
        .iter()
        .map(|entry| {
            let path_element = MPathElement::new(entry.name_bytes().to_vec())?;
            let check_entry = process_entry(git_lfs, repo, entry)?;
            Ok((path_element, check_entry))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    // Send this entry down for checking, then recurse into children. The checking task depends
    // on getting parents before children in order to avoid doing excess fsnode fetches
    tx.blocking_send(CheckNode {
        path: path.clone(),
        contents,
    })?;

    for entry in tree.iter() {
        if entry.kind() == Some(ObjectType::Tree) {
            let path_element = MPathElement::new(entry.name_bytes().to_vec())?;
            let obj = entry.to_object(repo)?;
            let subtree_path = RepoPath::dir(MPath::join_opt_element(path.mpath(), &path_element))?;
            let tree = obj.peel_to_tree()?;

            walk_tree(git_lfs, subtree_path, tx, repo, tree)?;
        }
    }

    Ok(())
}

/// Spawn a thread that reads the commit, and outputs a chain of `CheckNode`s to a channel
pub(crate) fn thread(
    repo: Repository,
    commit: String,
    git_lfs: bool,
    tx: mpsc::Sender<CheckNode>,
) -> Result<()> {
    let git_commit = repo.find_commit(Oid::from_str(&commit)?)?;

    let root = git_commit.tree()?;

    walk_tree(git_lfs, RepoPath::root(), &tx, &repo, root)
}

#[cfg(test)]
mod test {
    use super::*;

    // The content used in the LFS pointer
    const CONTENT: &[u8] = b"hello\n";
    // This is a real LFS pointer from `git lfs pointer`
    const LFS_POINTER: &[u8] = b"version https://git-lfs.github.com/spec/v1\noid sha256:5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03\nsize 6\n";
    // Damaged the hash - it's not a genuine SHA-256 hash
    const BROKEN_HASH_LFS_POINTER: &[u8] = b"version https://git-lfs.github.com/spec/v1\noid sha256:5MOO91b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03\nsize 6\n";
    // No hash in the LFS pointer
    const NO_HASH_LFS_POINTER: &[u8] = b"version https://git-lfs.github.com/spec/v1\n\nsize 6\n";
    // Not UTF-8
    const NOT_UTF8_LFS_POINTER: &[u8] =
        b"version https://git-lfs.github.com/spec/v1\n\x91a\nsize 6\n";
    // Bad LFS version
    const BAD_VERSION_LFS_POINTER: &[u8] = b"version https://git-lfs.github.com/spec/v2\noid sha256:5MOO91b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03\nsize 6\n";
    // Unknown hash type
    const BAD_HASH_TYPE_LFS_POINTER: &[u8] = b"version https://git-lfs.github.com/spec/v1\noid sha2-256:5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03\nsize 6\n";

    #[test]
    fn test_lfs_pointer() {
        // No LFS, hash of the pointer
        assert_eq!(
            get_sha256(false, LFS_POINTER),
            hash::Sha256::from_str(
                "94cb9a4fb124ed218aeeaefa7927680d5a261652f400f9d4f6a4e729c995d088",
            )
            .unwrap(),
        );
        // LFS, hash extracted from the pointer
        assert_eq!(
            get_sha256(true, LFS_POINTER),
            hash::Sha256::from_str(
                "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03",
            )
            .unwrap(),
        );
        // And check the LFS hash matches the real content
        assert_eq!(get_sha256(true, LFS_POINTER), get_sha256(true, CONTENT));
    }

    #[test]
    fn test_bad_pointers() {
        // All of these are bad pointers of some form, so we should get their hash, not extract a hash
        // from the pointer
        assert_eq!(
            get_sha256(true, BROKEN_HASH_LFS_POINTER),
            hash::Sha256::from_str(
                "8de25c66642bdcfed8e6b5f84dff6eda5ae6054a36c909a52c0ba8208d50ea09",
            )
            .unwrap(),
        );
        assert_ne!(
            get_sha256(true, BROKEN_HASH_LFS_POINTER),
            get_sha256(true, CONTENT)
        );
        assert_eq!(
            get_sha256(true, NO_HASH_LFS_POINTER),
            hash::Sha256::from_str(
                "e326ffde029b1b9101bbb00ca3da8f6e526685b5b1296e46b446a779e7c847a8",
            )
            .unwrap(),
        );
        assert_ne!(
            get_sha256(true, NO_HASH_LFS_POINTER),
            get_sha256(true, CONTENT)
        );
        assert_eq!(
            get_sha256(true, NOT_UTF8_LFS_POINTER),
            hash::Sha256::from_str(
                "328395baea3e9b372ac8040b66f2607ca208668623561d5da35a124b538a1e03",
            )
            .unwrap(),
        );
        assert_ne!(
            get_sha256(true, NOT_UTF8_LFS_POINTER),
            get_sha256(true, CONTENT)
        );
        assert_eq!(
            get_sha256(true, BAD_VERSION_LFS_POINTER),
            hash::Sha256::from_str(
                "3a2d968718c0ba448b90f2288985cbe158128e8d360505bf3f39eefd1f46b382",
            )
            .unwrap(),
        );
        assert_ne!(
            get_sha256(true, BAD_VERSION_LFS_POINTER),
            get_sha256(true, CONTENT)
        );
        assert_eq!(
            get_sha256(true, BAD_HASH_TYPE_LFS_POINTER),
            hash::Sha256::from_str(
                "37fb1ff2a48362f5c4d64c1b9121875a245a96f2787db5dd2f5c3273aabc4cb3",
            )
            .unwrap(),
        );
        assert_ne!(
            get_sha256(true, BAD_HASH_TYPE_LFS_POINTER),
            get_sha256(true, CONTENT)
        );
    }
}
