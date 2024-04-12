/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use types::RepoPath;

/// Content similarity threhold for rename detection. The definition of "similarity"
/// between file a and file b is: (len(a.lines()) - edit_cost(a, b)) / len(a.lines())
///   * 1.0 means exact match
///   * 0.0 means not match at all
/// The default value is 0.8.
const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.8;

/// Maximum rename edit cost determines whether we treat two files as a rename
const DEFAULT_MAX_EDIT_COST: u64 = 1000;

/// Contants for path similarity score. The actually value does not matter
/// here, we are more care about the ordering. Based on Git community's data,
/// ~70%+ of renames have the same basename:
/// https://github.com/git/git/blob/74cc1aa55f30ed76424a0e7226ab519aa6265061/diffcore-rename.c#L904-L907

/// Computes the similarity score between file paths.
///
/// The score will be in range [0.0, 1.0]. Higher number means more similar.
/// The algorithm here is a simple diff algorithm to calculate the similarity
/// bettween two file paths, which should cover below cases, and it will have
/// better result for common files like 'lib.rs', '__init__.py', etc.
///   - moving from one directory to antoher (same basename)
///   - moving inside a directory (same directory name)
pub(crate) fn file_path_similarity(p1: &RepoPath, p2: &RepoPath) -> f64 {
    let p1_chars: Vec<char> = p1.as_str().chars().collect();
    let p2_chars: Vec<char> = p2.as_str().chars().collect();

    let mut start = 0;
    while start < p1_chars.len() && start < p2_chars.len() && p1_chars[start] == p2_chars[start] {
        start += 1;
    }

    let mut end = 0;
    while end < p1_chars.len() - start
        && end < p2_chars.len() - start
        && p1_chars[p1_chars.len() - 1 - end] == p2_chars[p2_chars.len() - 1 - end]
    {
        end += 1;
    }

    (start + end) as f64 * 2.0 / (p1_chars.len() + p2_chars.len()) as f64
}

/// Check if two contents are considered similar based on the given config.
pub fn is_content_similar(a: &[u8], b: &[u8], config: &dyn Config) -> Result<bool> {
    let (similar, _) = content_similarity(a, b, config, Option::None)?;
    Ok(similar)
}

/// Return (is_similar, similarity score) pair between two contents.
///   - When is_similar is false, the similarity score is always 0.0. This is an optimization
///     to calculate similarity score only when necessary.
pub fn content_similarity(
    a: &[u8],
    b: &[u8],
    config: &dyn Config,
    threshold: Option<f32>,
) -> Result<(bool, f32)> {
    let config_threshold = config
        .get_opt::<f32>("copytrace", "similarity-threshold")?
        .unwrap_or(DEFAULT_SIMILARITY_THRESHOLD);
    tracing::trace!(?threshold, ?config_threshold, "content similarity");

    let threshold = threshold.unwrap_or(config_threshold);
    if threshold <= 0.0 {
        return Ok((true, 0.0));
    }

    let config_max_edit_cost = config
        .get_opt::<u64>("copytrace", "max-edit-cost")?
        .unwrap_or(DEFAULT_MAX_EDIT_COST);
    let mut lines = a.iter().filter(|&&c| c == b'\n').count();
    if lines == 0 {
        // avoid 'nan' when compute the similarity score
        lines += 1;
    }

    let max_edit_cost = config_max_edit_cost.min((lines as f32 * (1.0 - threshold)).round() as u64);
    let cost = xdiff::edit_cost(a, b, max_edit_cost + 1);

    tracing::trace!(
        ?threshold,
        ?config_max_edit_cost,
        ?lines,
        ?max_edit_cost,
        ?cost,
        "content similarity configs"
    );

    if cost <= max_edit_cost {
        let score = (lines as f32 - cost as f32) / lines as f32;
        tracing::trace!(?score, "content similarity score");
        Ok((true, score))
    } else {
        // For cost > max_edit_cost, we treat it as not similar and we don't care about
        // the acutal similarity score.
        Ok((false, 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_most_similar_path<'a>(candidates: &[&'a str], target: &'a str) -> &'a str {
        let mut candidates: Vec<&RepoPath> = candidates
            .iter()
            .map(|x| RepoPath::from_str(x).unwrap())
            .collect();
        let target = RepoPath::from_str(target).unwrap();
        candidates.sort_by_key(|path| {
            let score = (file_path_similarity(path, target) * 1000.0) as i32;
            (-score, path.to_owned())
        });
        let first = candidates.first().unwrap();
        first.as_str()
    }

    #[test]
    fn test_directory_move() {
        // rename 'edenscm' -> 'sapling'
        let candidates = vec![
            "ab/cd/edenscm/1/lib.rs",
            "ab/cd/edenscm/2/lib.rs",
            "ab/cd/edenscm/3/lib.rs",
            "ab/cd/edenscm/4/lib.rs",
            "ab/cd/edenscm/1/rename.rs",
            "a.txt",
        ];

        assert_eq!(
            get_most_similar_path(&candidates, "ab/cd/sapling/1/lib.rs"),
            "ab/cd/edenscm/1/lib.rs",
        );
        assert_eq!(
            get_most_similar_path(&candidates, "ab/cd/sapling/4/lib.rs"),
            "ab/cd/edenscm/4/lib.rs",
        );
        assert_eq!(
            get_most_similar_path(&candidates, "ab/cd/sapling/1/rename.rs"),
            "ab/cd/edenscm/1/rename.rs",
        );
        assert_eq!(get_most_similar_path(&candidates, "b.txt"), "a.txt",);
    }

    #[test]
    fn test_batch_moves() {
        // rename *.txt to *.md
        let candidates = vec!["a/b/4.txt", "a/b/1.txt", "a/b/2.txt", "a/b/3.txt"];

        assert_eq!(get_most_similar_path(&candidates, "a/b/1.md"), "a/b/1.txt",);
        assert_eq!(get_most_similar_path(&candidates, "a/b/2.md"), "a/b/2.txt",);
        assert_eq!(get_most_similar_path(&candidates, "a/b/3.md"), "a/b/3.txt",);
    }
}
