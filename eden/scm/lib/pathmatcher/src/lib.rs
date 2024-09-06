/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
pub use pathmatcher_types::IntersectMatcher;
pub use pathmatcher_types::Matcher;
pub use pathmatcher_types::NegateMatcher;
pub use pathmatcher_types::NeverMatcher;
pub use pathmatcher_types::UnionMatcher;
pub use pathmatcher_types::XorMatcher;

pub use crate::error::Error;
pub use crate::exact_matcher::ExactMatcher;
pub use crate::gitignore_matcher::GitignoreMatcher;
pub use crate::hinted_matcher::HintedMatcher;
pub use crate::matcher::build_matcher;
pub use crate::matcher::cli_matcher;
pub use crate::matcher::cli_matcher_with_filesets;
pub use crate::pattern::build_patterns;
pub use crate::pattern::split_pattern;
pub use crate::pattern::PatternKind;
pub use crate::regex_matcher::RegexMatcher;
pub use crate::tree_matcher::TreeMatcher;
pub use crate::utils::expand_curly_brackets;
pub use crate::utils::normalize_glob;
pub use crate::utils::plain_to_glob;

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use anyhow::Result;
    use types::RepoPath;

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
}
