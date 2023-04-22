/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use types::RepoPath;

/// Contants for path similarity score. The actually number does not matter
/// here, we are more care about the ordering. Based on Git community's data,
/// ~70%+ of renames have the same basename:
/// https://github.com/git/git/blob/74cc1aa55f30ed76424a0e7226ab519aa6265061/diffcore-rename.c#L904-L907
const PATH_SAME_BASENAME_SCORE: i8 = 70;
const PATH_SAME_DIRECTORY_SCORE: i8 = 20;
const PATH_DEFAULT_SCORE: i8 = 1;

/// Computes the similarity score between file paths.
///
/// The score will be in range [0, 100]. Higher number means more similar.
/// The algorithm here is based heuristics, we're assuming the renames are
/// of following two types in most cases:
///   - moving from one directory to antoher (same basename)
///   - moving inside a directory (same directory name)
#[allow(dead_code)]
pub(crate) fn file_path_similarity(p1: &RepoPath, p2: &RepoPath) -> i8 {
    let (dir1, basename1) = match p1.split_last_component() {
        None => return 0,
        Some(val) => val,
    };
    let (dir2, basename2) = match p2.split_last_component() {
        None => return 0,
        Some(val) => val,
    };

    if basename1 == basename2 {
        return PATH_SAME_BASENAME_SCORE;
    }
    if dir1 == dir2 {
        return PATH_SAME_DIRECTORY_SCORE;
    }
    PATH_DEFAULT_SCORE
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_file_path_similarity {
        ($p1:expr, $p2:expr, $expected:expr) => {{
            let p1 = RepoPath::from_str($p1).unwrap();
            let p2 = RepoPath::from_str($p2).unwrap();
            let actual = file_path_similarity(p1, p2);
            assert_eq!($expected, actual);
        }};
    }

    #[test]
    fn test_paths_with_same_basename() {
        assert_file_path_similarity!("a/1.txt", "1.txt", PATH_SAME_BASENAME_SCORE);
        assert_file_path_similarity!("a/1.txt", "b/1.txt", PATH_SAME_BASENAME_SCORE);
        assert_file_path_similarity!("a/b/1.txt", "b/1.txt", PATH_SAME_BASENAME_SCORE);
        assert_file_path_similarity!("a/b/1.txt", "b/c/d/1.txt", PATH_SAME_BASENAME_SCORE);
    }

    #[test]
    fn test_paths_with_same_directory() {
        assert_file_path_similarity!("1.txt", "2.txt", PATH_SAME_DIRECTORY_SCORE);
        assert_file_path_similarity!("a/1.txt", "a/2.txt", PATH_SAME_DIRECTORY_SCORE);
        assert_file_path_similarity!("a/b/1.txt", "a/b/2.txt", PATH_SAME_DIRECTORY_SCORE);
    }

    #[test]
    fn test_paths_without_same_basename_or_directory() {
        assert_file_path_similarity!("a/1.txt", "2.txt", PATH_DEFAULT_SCORE);
        assert_file_path_similarity!("a/1.txt", "b/2.txt", PATH_DEFAULT_SCORE);
    }
}
