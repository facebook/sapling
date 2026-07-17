/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod depth_matcher;
mod error;
mod exact_matcher;
mod gitignore_matcher;
mod hinted_matcher;
mod matcher;
mod pattern;
mod regex_matcher;
mod tree_matcher;
mod utils;

pub use pathmatcher_types::AlwaysMatcher;
pub use pathmatcher_types::DifferenceMatcher;
pub use pathmatcher_types::DirectoryMatch;
pub use pathmatcher_types::DynMatcher;
pub use pathmatcher_types::GraftMatcher;
pub use pathmatcher_types::IntersectMatcher;
pub use pathmatcher_types::Matcher;
pub use pathmatcher_types::NegateMatcher;
pub use pathmatcher_types::NeverMatcher;
pub use pathmatcher_types::UnionMatcher;
pub use pathmatcher_types::XorMatcher;
pub use types::RepoPath;

pub use crate::depth_matcher::DepthMatcher;
pub use crate::error::Error;
pub use crate::exact_matcher::ExactMatcher;
pub use crate::gitignore_matcher::GitignoreMatcher;
pub use crate::hinted_matcher::HintedMatcher;
pub use crate::matcher::build_matcher;
pub use crate::matcher::cli_matcher;
pub use crate::matcher::cli_matcher_with_filesets;
pub use crate::pattern::Pattern;
pub use crate::pattern::PatternKind;
pub use crate::pattern::build_patterns;
pub use crate::pattern::split_pattern;
pub use crate::regex_matcher::RegexMatcher;
pub use crate::tree_matcher::TreeMatcher;
pub use crate::utils::expand_curly_brackets;
pub use crate::utils::make_glob_recursive;
pub use crate::utils::normalize_glob;
pub use crate::utils::plain_to_glob;

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use anyhow::Result;

    use super::*;

    #[test]
    fn test_intersection_matcher() -> Result<()> {
        let empty = IntersectMatcher::new(Vec::new());
        assert_eq!(
            empty.matches_directory("something".try_into()?)?,
            DirectoryMatch::Nothing
        );
        assert!(!empty.matches_file("something".try_into()?)?);

        let matcher = IntersectMatcher::new(vec![
            Arc::new(ExactMatcher::new(
                [RepoPath::from_str("both/both")?, RepoPath::from_str("a/a")?].iter(),
                true,
            )),
            Arc::new(ExactMatcher::new(
                [RepoPath::from_str("both/both")?, RepoPath::from_str("b/b")?].iter(),
                true,
            )),
        ]);

        assert_eq!(
            matcher.matches_directory("both".try_into()?)?,
            DirectoryMatch::ShouldTraverse
        );
        assert_eq!(
            matcher.matches_directory("neither".try_into()?)?,
            DirectoryMatch::Nothing
        );
        assert_eq!(
            matcher.matches_directory("a".try_into()?)?,
            DirectoryMatch::Nothing
        );

        assert!(matcher.matches_file("both/both".try_into()?)?);
        assert!(!matcher.matches_file("neither".try_into()?)?);
        assert!(!matcher.matches_file("a/a".try_into()?)?);

        Ok(())
    }

    fn filter_matcher() -> Result<DynMatcher> {
        Ok(Arc::new(DifferenceMatcher::new(
            Arc::new(AlwaysMatcher::new()),
            Arc::new(ExactMatcher::new(
                [RepoPath::from_str("foo/secret")?].iter(),
                true,
            )),
        )))
    }

    #[test]
    fn test_graft_matcher_remaps_files() -> Result<()> {
        let matcher = GraftMatcher::new(
            filter_matcher()?,
            vec![(
                RepoPath::from_str("foo")?.to_owned(),
                RepoPath::from_str("bar")?.to_owned(),
            )],
        );

        assert!(!matcher.matches_file("foo/secret".try_into()?)?);
        assert!(!matcher.matches_file("bar/secret".try_into()?)?);
        assert!(matcher.matches_file("bar/public".try_into()?)?);
        Ok(())
    }

    #[test]
    fn test_graft_matcher_traverses_graft_destination_ancestors() -> Result<()> {
        let matcher = GraftMatcher::new(
            filter_matcher()?,
            vec![(
                RepoPath::from_str("foo")?.to_owned(),
                RepoPath::from_str("a/b")?.to_owned(),
            )],
        );

        assert_eq!(
            matcher.matches_directory("".try_into()?)?,
            DirectoryMatch::ShouldTraverse
        );
        assert_eq!(
            matcher.matches_directory("a".try_into()?)?,
            DirectoryMatch::ShouldTraverse
        );
        assert_eq!(
            matcher.matches_directory("a/b".try_into()?)?,
            DirectoryMatch::ShouldTraverse
        );
        assert!(!matcher.matches_file("a/b/secret".try_into()?)?);
        assert!(matcher.matches_file("a/b/public".try_into()?)?);
        Ok(())
    }
}
