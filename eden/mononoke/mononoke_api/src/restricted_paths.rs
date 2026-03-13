/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::NonRootMPath;
use restricted_paths::PathRestrictionInfo;

/// Access check result for a restricted path.
#[derive(Clone, Debug, PartialEq)]
pub struct PathAccessInfo {
    /// Core restriction info from the restricted_paths crate.
    pub restriction: PathRestrictionInfo,

    /// Whether the caller has access. None if not checked.
    pub has_access: Option<bool>,
}

impl PathAccessInfo {
    /// Convenience accessor for the restriction root.
    pub fn restriction_root(&self) -> &NonRootMPath {
        &self.restriction.restriction_root
    }

    /// Convenience accessor for the repo region ACL.
    pub fn repo_region_acl(&self) -> &str {
        &self.restriction.repo_region_acl
    }

    /// Convenience accessor for the request ACL.
    pub fn request_acl(&self) -> &str {
        &self.restriction.request_acl
    }
}

/// Information about restricted path changes in a changeset.
#[derive(Clone, Debug, PartialEq)]
pub struct RestrictedPathsChangesInfo {
    /// Changed paths that fall under restrictions, grouped by restriction root.
    pub restricted_changes: Vec<RestrictedChangeGroup>,
}

/// A group of changed paths that share the same restriction root.
#[derive(Clone, Debug, PartialEq)]
pub struct RestrictedChangeGroup {
    /// The restriction root and access info covering these changes.
    pub restriction_info: PathAccessInfo,
    // TODO(T248660146): remove this field and `RestrictedChangeGroup` if there's
    // no need to use it for now.
    /// The changed paths under this restriction root.
    pub changed_paths: Vec<NonRootMPath>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;

    use anyhow::Result;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use metaconfig_types::RestrictedPathsConfig;
    use mononoke_macros::mononoke;
    use mononoke_types::RepositoryId;
    use mononoke_types::path::MPath;
    use permission_checker::MononokeIdentity;
    use permission_checker::dummy::DummyAclProvider;
    use repo_derived_data::RepoDerivedDataArc;
    use restricted_paths::RestrictedPaths;
    use restricted_paths::RestrictedPathsConfigBased;
    use restricted_paths::SqlRestrictedPathsManifestIdStoreBuilder;
    use scuba_ext::MononokeScubaSampleBuilder;
    use sql_construct::SqlConstruct;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::changeset::ChangesetContext;
    use crate::repo::Repo;
    use crate::repo::RepoContext;

    /// Create RestrictedPaths with the given path ACLs for testing.
    async fn create_test_restricted_paths(
        fb: FacebookInit,
        path_acls: Vec<(&str, &str)>,
    ) -> Result<Arc<RestrictedPaths>> {
        let repo_id = RepositoryId::new(0);

        let path_acls_map: HashMap<NonRootMPath, MononokeIdentity> = path_acls
            .into_iter()
            .map(|(path, acl_str)| -> Result<_> {
                Ok((
                    NonRootMPath::new(path)?,
                    MononokeIdentity::from_str(acl_str)?,
                ))
            })
            .collect::<Result<_>>()?;

        let config = RestrictedPathsConfig {
            path_acls: path_acls_map,
            use_manifest_id_cache: false,
            cache_update_interval_ms: 100,
            soft_path_acls: Vec::new(),
            tooling_allowlist_group: None,
            conditional_enforcement_acls: Vec::new(),
            acl_file_name: RestrictedPathsConfig::default().acl_file_name,
        };

        let manifest_id_store = Arc::new(
            SqlRestrictedPathsManifestIdStoreBuilder::with_sqlite_in_memory()?
                .with_repo_id(repo_id),
        );

        let acl_provider = DummyAclProvider::new(fb)?;
        let scuba = MononokeScubaSampleBuilder::with_discard();

        let config_based = Arc::new(RestrictedPathsConfigBased::new(
            config,
            manifest_id_store,
            None,
        ));

        // Build a minimal repo to get ArcRepoDerivedData
        let test_repo: Repo = TestRepoFactory::new(fb)?.build().await?;
        let repo_derived_data = test_repo.repo_derived_data_arc();

        Ok(Arc::new(RestrictedPaths::new(
            config_based,
            acl_provider,
            scuba,
            false, // use_acl_manifest — config-based tests; separate repo lacks commit graph
            repo_derived_data,
        )?))
    }

    /// Create a RepoContext and ChangesetContext with restricted paths configured.
    async fn create_test_changeset(
        fb: FacebookInit,
        path_acls: Vec<(&str, &str)>,
    ) -> Result<(RepoContext<Repo>, ChangesetContext<Repo>)> {
        let restricted_paths = create_test_restricted_paths(fb, path_acls).await?;
        let ctx = CoreContext::test_mock(fb);

        let repo: Repo = TestRepoFactory::new(fb)?
            .with_restricted_paths(restricted_paths)
            .build()
            .await?;

        let root_cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "content")
            .commit()
            .await?;

        let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
        let cs_ctx = ChangesetContext::new(repo_ctx.clone(), root_cs_id);

        Ok((repo_ctx, cs_ctx))
    }

    // ---- restriction_info tests ----

    #[mononoke::fbinit_test]
    async fn test_restriction_info_exact_match(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("restricted/dir", "TIER:my-acl")]).await?;

        let info = cs_ctx
            .path_restriction(MPath::try_from("restricted/dir")?)
            .await?
            .restriction_info(true)
            .await?;

        let expected = vec![PathAccessInfo {
            restriction: PathRestrictionInfo {
                restriction_root: NonRootMPath::new("restricted/dir")?,
                repo_region_acl: "TIER:my-acl".to_string(),
                request_acl: "TIER:my-acl".to_string(),
            },
            has_access: Some(true),
        }];
        let mut actual = info;
        actual.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restriction_info_nested_path(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("restricted", "TIER:my-acl")]).await?;

        let info = cs_ctx
            .path_restriction(MPath::try_from("restricted/subdir/file.txt")?)
            .await?
            .restriction_info(true)
            .await?;

        let expected = vec![PathAccessInfo {
            restriction: PathRestrictionInfo {
                restriction_root: NonRootMPath::new("restricted")?,
                repo_region_acl: "TIER:my-acl".to_string(),
                request_acl: "TIER:my-acl".to_string(),
            },
            has_access: Some(true),
        }];
        let mut actual = info;
        actual.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restriction_info_no_match(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("restricted/dir", "TIER:my-acl")]).await?;

        let info = cs_ctx
            .path_restriction(MPath::try_from("other/path/file.txt")?)
            .await?
            .restriction_info(true)
            .await?;

        assert!(info.is_empty(), "path should not be restricted");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restriction_info_sibling_path(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("foo/bar", "TIER:my-acl")]).await?;

        // foo/baz is a sibling of foo/bar, not under it
        let info = cs_ctx
            .path_restriction(MPath::try_from("foo/baz/file.txt")?)
            .await?
            .restriction_info(true)
            .await?;

        assert!(info.is_empty(), "sibling path should not be restricted");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restriction_info_multiple_restrictions(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) = create_test_changeset(
            fb,
            vec![("first", "TIER:first-acl"), ("second", "TIER:second-acl")],
        )
        .await?;

        let first_info = cs_ctx
            .path_restriction(MPath::try_from("first/nested/file.txt")?)
            .await?
            .restriction_info(true)
            .await?;

        let expected_first = vec![PathAccessInfo {
            restriction: PathRestrictionInfo {
                restriction_root: NonRootMPath::new("first")?,
                repo_region_acl: "TIER:first-acl".to_string(),
                request_acl: "TIER:first-acl".to_string(),
            },
            has_access: Some(true),
        }];
        let mut actual_first = first_info;
        actual_first.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual_first, expected_first);

        let second_info = cs_ctx
            .path_restriction(MPath::try_from("second/nested/file.txt")?)
            .await?
            .restriction_info(true)
            .await?;

        let expected_second = vec![PathAccessInfo {
            restriction: PathRestrictionInfo {
                restriction_root: NonRootMPath::new("second")?,
                repo_region_acl: "TIER:second-acl".to_string(),
                request_acl: "TIER:second-acl".to_string(),
            },
            has_access: Some(true),
        }];
        let mut actual_second = second_info;
        actual_second.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual_second, expected_second);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restriction_info_root_path(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("restricted", "TIER:my-acl")]).await?;

        // Root path cannot be restricted
        let info = cs_ctx
            .path_restriction(MPath::ROOT)
            .await?
            .restriction_info(true)
            .await?;

        assert!(info.is_empty(), "root path cannot be restricted");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restriction_info_no_restrictions_configured(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) = create_test_changeset(fb, vec![]).await?;

        let info = cs_ctx
            .path_restriction(MPath::try_from("any/path/file.txt")?)
            .await?
            .restriction_info(true)
            .await?;

        assert!(info.is_empty(), "no restrictions configured");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restriction_info_nested_roots(fb: FacebookInit) -> Result<()> {
        // A path can fall under multiple nested tent roots.
        // e.g. foo/ has acl_a and foo/bar/ has acl_b, querying foo/bar/file.txt
        // should return both roots. See comment in D93601751.
        let (_repo_ctx, cs_ctx) = create_test_changeset(
            fb,
            vec![("foo", "TIER:outer-acl"), ("foo/bar", "TIER:inner-acl")],
        )
        .await?;

        let infos = cs_ctx
            .path_restriction(MPath::try_from("foo/bar/file.txt")?)
            .await?
            .restriction_info(true)
            .await?;

        let expected = vec![
            PathAccessInfo {
                restriction: PathRestrictionInfo {
                    restriction_root: NonRootMPath::new("foo")?,
                    repo_region_acl: "TIER:outer-acl".to_string(),
                    request_acl: "TIER:outer-acl".to_string(),
                },
                has_access: Some(true),
            },
            PathAccessInfo {
                restriction: PathRestrictionInfo {
                    restriction_root: NonRootMPath::new("foo/bar")?,
                    repo_region_acl: "TIER:inner-acl".to_string(),
                    request_acl: "TIER:inner-acl".to_string(),
                },
                has_access: Some(true),
            },
        ];
        let mut actual = infos;
        actual.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    // ---- paths_restriction_info batch tests ----

    #[mononoke::fbinit_test]
    async fn test_batch_restriction_info_mixed_paths(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) = create_test_changeset(
            fb,
            vec![
                ("first", "TIER:first-acl"),
                ("second", "TIER:second-acl"),
                ("third/nested", "TIER:third-acl"),
            ],
        )
        .await?;

        let paths = vec![
            NonRootMPath::new("first/file1.txt")?,
            NonRootMPath::new("second/subdir/file2.txt")?,
            NonRootMPath::new("third/nested/deep/file3.txt")?,
            NonRootMPath::new("unrestricted/file4.txt")?,
        ];

        let results = cs_ctx.paths_restriction_info(paths, true).await?;

        // Build comparable (path, Option<(restriction_root, repo_region_acl)>) tuples
        // Using first() since these paths each have at most one matching root
        let mut actual: Vec<(String, Option<(String, String)>)> = results
            .into_iter()
            .map(|(path, infos)| {
                (
                    path.to_string(),
                    infos.first().map(|i| {
                        (
                            i.restriction_root().to_string(),
                            i.repo_region_acl().to_string(),
                        )
                    }),
                )
            })
            .collect();
        actual.sort_by(|a, b| a.0.cmp(&b.0));

        let expected: Vec<(String, Option<(String, String)>)> = vec![
            (
                "first/file1.txt".to_string(),
                Some(("first".to_string(), "TIER:first-acl".to_string())),
            ),
            (
                "second/subdir/file2.txt".to_string(),
                Some(("second".to_string(), "TIER:second-acl".to_string())),
            ),
            (
                "third/nested/deep/file3.txt".to_string(),
                Some(("third/nested".to_string(), "TIER:third-acl".to_string())),
            ),
            ("unrestricted/file4.txt".to_string(), None),
        ];

        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_batch_all_restricted(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("restricted", "TIER:my-acl")]).await?;

        let paths = vec![
            NonRootMPath::new("restricted/file1.txt")?,
            NonRootMPath::new("restricted/file2.txt")?,
            NonRootMPath::new("restricted/subdir/file3.txt")?,
        ];

        let results = cs_ctx.paths_restriction_info(paths, true).await?;

        let mut actual: Vec<(String, Option<(String, String)>)> = results
            .into_iter()
            .map(|(path, infos)| {
                (
                    path.to_string(),
                    infos.first().map(|i| {
                        (
                            i.restriction_root().to_string(),
                            i.repo_region_acl().to_string(),
                        )
                    }),
                )
            })
            .collect();
        actual.sort_by(|a, b| a.0.cmp(&b.0));

        let expected: Vec<(String, Option<(String, String)>)> = vec![
            (
                "restricted/file1.txt".to_string(),
                Some(("restricted".to_string(), "TIER:my-acl".to_string())),
            ),
            (
                "restricted/file2.txt".to_string(),
                Some(("restricted".to_string(), "TIER:my-acl".to_string())),
            ),
            (
                "restricted/subdir/file3.txt".to_string(),
                Some(("restricted".to_string(), "TIER:my-acl".to_string())),
            ),
        ];

        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_batch_all_unrestricted(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("restricted", "TIER:my-acl")]).await?;

        let paths = vec![
            NonRootMPath::new("unrestricted/file1.txt")?,
            NonRootMPath::new("other/file2.txt")?,
        ];

        let results = cs_ctx.paths_restriction_info(paths, true).await?;

        let mut actual: Vec<(String, Option<(String, String)>)> = results
            .into_iter()
            .map(|(path, infos)| {
                (
                    path.to_string(),
                    infos.first().map(|i| {
                        (
                            i.restriction_root().to_string(),
                            i.repo_region_acl().to_string(),
                        )
                    }),
                )
            })
            .collect();
        actual.sort_by(|a, b| a.0.cmp(&b.0));

        let expected: Vec<(String, Option<(String, String)>)> = vec![
            ("other/file2.txt".to_string(), None),
            ("unrestricted/file1.txt".to_string(), None),
        ];

        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    // ---- find_restricted_descendants tests ----

    #[mononoke::fbinit_test]
    async fn test_find_descendants_no_restrictions(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) = create_test_changeset(fb, vec![]).await?;

        let descendants = cs_ctx
            .path_restriction(MPath::ROOT)
            .await?
            .find_restricted_descendants()
            .await?;

        assert!(descendants.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_descendants_root_returns_all(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) = create_test_changeset(
            fb,
            vec![
                ("first/path", "TIER:first-acl"),
                ("second/path", "TIER:second-acl"),
            ],
        )
        .await?;

        let descendants = cs_ctx
            .path_restriction(MPath::ROOT)
            .await?
            .find_restricted_descendants()
            .await?;

        let expected = vec![
            PathAccessInfo {
                restriction: PathRestrictionInfo {
                    restriction_root: NonRootMPath::new("first/path")?,
                    repo_region_acl: "TIER:first-acl".to_string(),
                    request_acl: "TIER:first-acl".to_string(),
                },
                has_access: None,
            },
            PathAccessInfo {
                restriction: PathRestrictionInfo {
                    restriction_root: NonRootMPath::new("second/path")?,
                    repo_region_acl: "TIER:second-acl".to_string(),
                    request_acl: "TIER:second-acl".to_string(),
                },
                has_access: None,
            },
        ];
        let mut actual = descendants;
        actual.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_descendants_filter_exact_match(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) = create_test_changeset(
            fb,
            vec![
                ("first/path", "TIER:first-acl"),
                ("second/path", "TIER:second-acl"),
            ],
        )
        .await?;

        let descendants = cs_ctx
            .path_restriction(MPath::try_from("first/path")?)
            .await?
            .find_restricted_descendants()
            .await?;

        // "first/path" is itself a restriction root, and is_prefix_of returns
        // true for equal paths, so it should be returned
        let expected = vec![PathAccessInfo {
            restriction: PathRestrictionInfo {
                restriction_root: NonRootMPath::new("first/path")?,
                repo_region_acl: "TIER:first-acl".to_string(),
                request_acl: "TIER:first-acl".to_string(),
            },
            has_access: None,
        }];
        let mut actual = descendants;
        actual.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_descendants_filter_parent_of_root(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("foo/bar/restricted", "TIER:my-acl")]).await?;

        let descendants = cs_ctx
            .path_restriction(MPath::try_from("foo")?)
            .await?
            .find_restricted_descendants()
            .await?;

        let expected = vec![PathAccessInfo {
            restriction: PathRestrictionInfo {
                restriction_root: NonRootMPath::new("foo/bar/restricted")?,
                repo_region_acl: "TIER:my-acl".to_string(),
                request_acl: "TIER:my-acl".to_string(),
            },
            has_access: None,
        }];
        let mut actual = descendants;
        actual.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_descendants_filter_child_of_root_returns_empty(
        fb: FacebookInit,
    ) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("foo/bar", "TIER:my-acl")]).await?;

        // Filter is a child of the root — should NOT match because we only
        // return roots that are under the filter
        let descendants = cs_ctx
            .path_restriction(MPath::try_from("foo/bar/baz/deep")?)
            .await?
            .find_restricted_descendants()
            .await?;

        assert!(descendants.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_descendants_filter_no_match(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) = create_test_changeset(
            fb,
            vec![
                ("first/path", "TIER:first-acl"),
                ("second/path", "TIER:second-acl"),
            ],
        )
        .await?;

        let descendants = cs_ctx
            .path_restriction(MPath::try_from("third/path")?)
            .await?
            .find_restricted_descendants()
            .await?;

        assert!(descendants.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_descendants_filter_sibling_no_match(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("foo/bar", "TIER:my-acl")]).await?;

        let descendants = cs_ctx
            .path_restriction(MPath::try_from("foo/baz")?)
            .await?
            .find_restricted_descendants()
            .await?;

        assert!(descendants.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_descendants_nested_roots(fb: FacebookInit) -> Result<()> {
        // Nested restriction roots: foo/ and foo/bar/
        let (_repo_ctx, cs_ctx) = create_test_changeset(
            fb,
            vec![("foo", "TIER:outer-acl"), ("foo/bar", "TIER:inner-acl")],
        )
        .await?;

        // Querying from foo/ should return both the outer and inner roots
        let descendants = cs_ctx
            .path_restriction(MPath::try_from("foo")?)
            .await?
            .find_restricted_descendants()
            .await?;

        let expected = vec![
            PathAccessInfo {
                restriction: PathRestrictionInfo {
                    restriction_root: NonRootMPath::new("foo")?,
                    repo_region_acl: "TIER:outer-acl".to_string(),
                    request_acl: "TIER:outer-acl".to_string(),
                },
                has_access: None,
            },
            PathAccessInfo {
                restriction: PathRestrictionInfo {
                    restriction_root: NonRootMPath::new("foo/bar")?,
                    repo_region_acl: "TIER:inner-acl".to_string(),
                    request_acl: "TIER:inner-acl".to_string(),
                },
                has_access: None,
            },
        ];
        let mut actual = descendants;
        actual.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    // ---- batch find_restricted_descendants tests ----

    #[mononoke::fbinit_test]
    async fn test_batch_find_descendants_multiple_roots(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) = create_test_changeset(
            fb,
            vec![
                ("first/path", "TIER:first-acl"),
                ("second/path", "TIER:second-acl"),
                ("third/path", "TIER:third-acl"),
            ],
        )
        .await?;

        let roots = vec![
            MPath::try_from("first/path")?,
            MPath::try_from("third/path")?,
        ];

        let descendants = cs_ctx.find_restricted_descendants(roots).await?;

        let expected = vec![
            PathAccessInfo {
                restriction: PathRestrictionInfo {
                    restriction_root: NonRootMPath::new("first/path")?,
                    repo_region_acl: "TIER:first-acl".to_string(),
                    request_acl: "TIER:first-acl".to_string(),
                },
                has_access: None,
            },
            PathAccessInfo {
                restriction: PathRestrictionInfo {
                    restriction_root: NonRootMPath::new("third/path")?,
                    repo_region_acl: "TIER:third-acl".to_string(),
                    request_acl: "TIER:third-acl".to_string(),
                },
                has_access: None,
            },
        ];
        let mut actual = descendants;
        actual.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_batch_find_descendants_deduplicates(fb: FacebookInit) -> Result<()> {
        let (_repo_ctx, cs_ctx) =
            create_test_changeset(fb, vec![("shared/path", "TIER:my-acl")]).await?;

        // Both roots are parents of the same restriction root
        let roots = vec![MPath::try_from("shared")?, MPath::ROOT];

        let descendants = cs_ctx.find_restricted_descendants(roots).await?;

        // Should be deduplicated to just one entry
        let expected = vec![PathAccessInfo {
            restriction: PathRestrictionInfo {
                restriction_root: NonRootMPath::new("shared/path")?,
                repo_region_acl: "TIER:my-acl".to_string(),
                request_acl: "TIER:my-acl".to_string(),
            },
            has_access: None,
        }];
        let mut actual = descendants;
        actual.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_batch_find_descendants_nested_roots_and_queries(fb: FacebookInit) -> Result<()> {
        // Nested restriction roots
        let (_repo_ctx, cs_ctx) = create_test_changeset(
            fb,
            vec![
                ("foo", "TIER:outer-acl"),
                ("foo/bar", "TIER:inner-acl"),
                ("baz", "TIER:baz-acl"),
            ],
        )
        .await?;

        // Nested query paths: foo/ is a parent of foo/bar/.
        // foo/ should find both foo/ and foo/bar/.
        // foo/bar/ should find just foo/bar/ (already found by foo/).
        // After dedup: foo/ and foo/bar/ (baz/ not matched by either query).
        let roots = vec![MPath::try_from("foo")?, MPath::try_from("foo/bar")?];

        let descendants = cs_ctx.find_restricted_descendants(roots).await?;

        let expected = vec![
            PathAccessInfo {
                restriction: PathRestrictionInfo {
                    restriction_root: NonRootMPath::new("foo")?,
                    repo_region_acl: "TIER:outer-acl".to_string(),
                    request_acl: "TIER:outer-acl".to_string(),
                },
                has_access: None,
            },
            PathAccessInfo {
                restriction: PathRestrictionInfo {
                    restriction_root: NonRootMPath::new("foo/bar")?,
                    repo_region_acl: "TIER:inner-acl".to_string(),
                    request_acl: "TIER:inner-acl".to_string(),
                },
                has_access: None,
            },
        ];
        let mut actual = descendants;
        actual.sort_by(|a, b| a.restriction_root().cmp(b.restriction_root()));
        pretty_assertions::assert_eq!(actual, expected);

        Ok(())
    }

    // ---- restricted_paths_changes tests ----

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_changes_with_restricted_files(fb: FacebookInit) -> Result<()> {
        let restricted_paths =
            create_test_restricted_paths(fb, vec![("restricted", "TIER:my-acl")]).await?;
        let ctx = CoreContext::test_mock(fb);

        let repo: Repo = TestRepoFactory::new(fb)?
            .with_restricted_paths(restricted_paths)
            .build()
            .await?;

        let root_cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("restricted/file.txt", "secret")
            .add_file("public/file.txt", "public")
            .commit()
            .await?;

        let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
        let cs_ctx = ChangesetContext::new(repo_ctx, root_cs_id);

        let changes = cs_ctx.restricted_paths_changes(false).await?;

        let expected = RestrictedPathsChangesInfo {
            restricted_changes: vec![RestrictedChangeGroup {
                restriction_info: PathAccessInfo {
                    restriction: PathRestrictionInfo {
                        restriction_root: NonRootMPath::new("restricted")?,
                        repo_region_acl: "TIER:my-acl".to_string(),
                        request_acl: "TIER:my-acl".to_string(),
                    },
                    has_access: None,
                },
                changed_paths: vec![NonRootMPath::new("restricted/file.txt")?],
            }],
        };
        pretty_assertions::assert_eq!(changes, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_changes_no_restricted_files(fb: FacebookInit) -> Result<()> {
        let restricted_paths =
            create_test_restricted_paths(fb, vec![("restricted", "TIER:my-acl")]).await?;
        let ctx = CoreContext::test_mock(fb);

        let repo: Repo = TestRepoFactory::new(fb)?
            .with_restricted_paths(restricted_paths)
            .build()
            .await?;

        let root_cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("public/file.txt", "public")
            .commit()
            .await?;

        let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
        let cs_ctx = ChangesetContext::new(repo_ctx, root_cs_id);

        let changes = cs_ctx.restricted_paths_changes(false).await?;

        assert!(changes.restricted_changes.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_changes_nested_roots(fb: FacebookInit) -> Result<()> {
        // Use nested roots: "first" contains "first/second"
        // A file under first/second/ should appear in both groups.
        let restricted_paths = create_test_restricted_paths(
            fb,
            vec![
                ("first", "TIER:first-acl"),
                ("first/second", "TIER:second-acl"),
            ],
        )
        .await?;
        let ctx = CoreContext::test_mock(fb);

        let repo: Repo = TestRepoFactory::new(fb)?
            .with_restricted_paths(restricted_paths)
            .build()
            .await?;

        let root_cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("first/a.txt", "a")
            .add_file("first/b.txt", "b")
            .add_file("first/second/c.txt", "c")
            .add_file("public/d.txt", "d")
            .commit()
            .await?;

        let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
        let cs_ctx = ChangesetContext::new(repo_ctx, root_cs_id);

        let changes = cs_ctx.restricted_paths_changes(false).await?;

        // Groups are sorted by restriction_root (BTreeMap ordering)
        // First group has 3 changed paths (a.txt, b.txt, second/c.txt)
        // because first/second/c.txt is also under first/
        // Second group has 1 changed path (first/second/c.txt)
        let expected = RestrictedPathsChangesInfo {
            restricted_changes: vec![
                RestrictedChangeGroup {
                    restriction_info: PathAccessInfo {
                        restriction: PathRestrictionInfo {
                            restriction_root: NonRootMPath::new("first")?,
                            repo_region_acl: "TIER:first-acl".to_string(),
                            request_acl: "TIER:first-acl".to_string(),
                        },
                        has_access: None,
                    },
                    changed_paths: vec![
                        NonRootMPath::new("first/a.txt")?,
                        NonRootMPath::new("first/b.txt")?,
                        NonRootMPath::new("first/second/c.txt")?,
                    ],
                },
                RestrictedChangeGroup {
                    restriction_info: PathAccessInfo {
                        restriction: PathRestrictionInfo {
                            restriction_root: NonRootMPath::new("first/second")?,
                            repo_region_acl: "TIER:second-acl".to_string(),
                            request_acl: "TIER:second-acl".to_string(),
                        },
                        has_access: None,
                    },
                    changed_paths: vec![NonRootMPath::new("first/second/c.txt")?],
                },
            ],
        };
        pretty_assertions::assert_eq!(changes, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_changes_multiple_unrelated_roots(
        fb: FacebookInit,
    ) -> Result<()> {
        // Two independent restriction roots with no nesting relationship
        let restricted_paths = create_test_restricted_paths(
            fb,
            vec![("alpha", "TIER:alpha-acl"), ("beta", "TIER:beta-acl")],
        )
        .await?;
        let ctx = CoreContext::test_mock(fb);

        let repo: Repo = TestRepoFactory::new(fb)?
            .with_restricted_paths(restricted_paths)
            .build()
            .await?;

        let root_cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("alpha/file1.txt", "a1")
            .add_file("alpha/file2.txt", "a2")
            .add_file("beta/file3.txt", "b1")
            .add_file("public/file4.txt", "p1")
            .commit()
            .await?;

        let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
        let cs_ctx = ChangesetContext::new(repo_ctx, root_cs_id);

        let changes = cs_ctx.restricted_paths_changes(false).await?;

        // Groups are sorted by restriction_root (BTreeMap ordering):
        // alpha/ has 2 changed paths, beta/ has 1 changed path.
        // public/file4.txt is unrestricted and should not appear.
        let expected = RestrictedPathsChangesInfo {
            restricted_changes: vec![
                RestrictedChangeGroup {
                    restriction_info: PathAccessInfo {
                        restriction: PathRestrictionInfo {
                            restriction_root: NonRootMPath::new("alpha")?,
                            repo_region_acl: "TIER:alpha-acl".to_string(),
                            request_acl: "TIER:alpha-acl".to_string(),
                        },
                        has_access: None,
                    },
                    changed_paths: vec![
                        NonRootMPath::new("alpha/file1.txt")?,
                        NonRootMPath::new("alpha/file2.txt")?,
                    ],
                },
                RestrictedChangeGroup {
                    restriction_info: PathAccessInfo {
                        restriction: PathRestrictionInfo {
                            restriction_root: NonRootMPath::new("beta")?,
                            repo_region_acl: "TIER:beta-acl".to_string(),
                            request_acl: "TIER:beta-acl".to_string(),
                        },
                        has_access: None,
                    },
                    changed_paths: vec![NonRootMPath::new("beta/file3.txt")?],
                },
            ],
        };
        pretty_assertions::assert_eq!(changes, expected);

        Ok(())
    }
}
