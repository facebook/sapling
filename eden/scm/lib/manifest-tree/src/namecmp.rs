/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Functions to compare path components.
//! Decide the ordering of items in a tree.

use std::cmp::Ordering;

use storemodel::TreeFormat;
use types::PathComponent;

use crate::Flag;

type NameCmpFunc = fn(&PathComponent, Flag, &PathComponent, Flag) -> Ordering;

pub(crate) fn get_namecmp_func(format: TreeFormat) -> NameCmpFunc {
    match format {
        TreeFormat::Git => namecmp_git,
        TreeFormat::Hg => namecmp_hg,
    }
}

pub(crate) fn namecmp_hg(
    name1: &PathComponent,
    flag1: Flag,
    name2: &PathComponent,
    flag2: Flag,
) -> Ordering {
    (name1, flag1).cmp(&(name2, flag2))
}

/// Compare names for git.
/// Git treats directory names differently by appending `/` to them.
pub(crate) fn namecmp_git(
    name1: &PathComponent,
    flag1: Flag,
    name2: &PathComponent,
    flag2: Flag,
) -> Ordering {
    let name1 = name1.as_str().as_bytes();
    let name2 = name2.as_str().as_bytes();

    // This is basically base_name_compare() authored by Linus in
    // https://github.com/git/git/commit/958ba6c96eb58b359c855c9d07e3e45072f0503e
    let len = name1.len().min(name2.len());
    let cmp = name1[..len].cmp(&name2[..len]);
    if cmp != Ordering::Equal {
        return cmp;
    }

    let end = |flag| if flag == Flag::Directory { &b'/' } else { &0 };
    let end1 = name1.get(len).unwrap_or_else(|| end(flag1));
    let end2 = name2.get(len).unwrap_or_else(|| end(flag2));

    end1.cmp(end2)
}

#[cfg(test)]
pub(crate) mod tests {
    use manifest::FileType;
    use types::PathComponentBuf;

    use super::*;

    #[test]
    fn test_namecmp_git() {
        let names = ["a", "b", "b-", "b0", "b/", "b-/", "b0/", "c"];
        for s1 in names.iter() {
            for s2 in names.iter() {
                check_namecmp_git(s1, s2);
            }
        }
    }

    /// s1 and s2 might end with '/' to indicate they are trees.
    fn check_namecmp_git(s1: &str, s2: &str) {
        let expected = s1.cmp(s2);

        let (name1, flag1) = get_name_flag(s1);
        let (name2, flag2) = get_name_flag(s2);

        let cmp = get_namecmp_func(TreeFormat::Git);
        let actual = cmp(&name1, flag1, &name2, flag2);
        assert_eq!(actual, expected);
    }

    /// Convenient function to get "PathComponentBuf" and "Flag" from a
    /// string that might ends with '/' indicating a directory.
    pub(crate) fn get_name_flag(name: &str) -> (PathComponentBuf, Flag) {
        let flag = if name.ends_with('/') {
            Flag::Directory
        } else {
            Flag::File(FileType::Regular)
        };
        let name = PathComponentBuf::from_string(name.trim_end_matches('/').to_string()).unwrap();
        (name, flag)
    }
}
