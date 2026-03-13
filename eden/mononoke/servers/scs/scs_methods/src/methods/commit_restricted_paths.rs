/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::Context;
use anyhow::Result;
use futures::StreamExt;
use futures::stream::BoxStream;
use mononoke_api::MononokeError;
use mononoke_api::Repo;
use mononoke_api::changeset::ChangesetContext;
use mononoke_types::NonRootMPath;
use mononoke_types::path::MPath;
use source_control as thrift;

pub(crate) async fn restricted_paths_access_impl(
    cs_ctx: &ChangesetContext<Repo>,
    paths: BTreeSet<String>,
    check_permissions: bool,
) -> Result<thrift::CommitRestrictedPathsAccessResponse, scs_errors::ServiceError> {
    // If no restricted paths configured, return empty response
    if !cs_ctx.has_restricted_paths() {
        return Ok(thrift::CommitRestrictedPathsAccessResponse {
            are_restricted: thrift::PathCoverage::NONE,
            has_access: thrift::PathCoverage::ALL,
            restriction_roots: BTreeMap::new(),
            authorized_paths: if check_permissions {
                paths.into_iter().collect()
            } else {
                vec![]
            },
            ..Default::default()
        });
    }

    let non_root_paths: Vec<NonRootMPath> = paths
        .iter()
        .map(|p| {
            NonRootMPath::try_from(p.as_str())
                .with_context(|| format!("Casting path {p} to NonRootMPath"))
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(|err| MononokeError::InternalError(err.into()))?;

    let restriction_results = cs_ctx
        .paths_restriction_info(non_root_paths, check_permissions)
        .await?;

    struct AggregatedResult {
        is_restricted: Vec<bool>,
        has_access: Vec<bool>,
        authorized_paths: Vec<String>,
        restriction_roots: BTreeMap<String, Vec<thrift::PathRestrictionRoot>>,
    }

    let result = restriction_results.iter().fold(
        AggregatedResult {
            is_restricted: Vec::with_capacity(restriction_results.len()),
            has_access: Vec::with_capacity(restriction_results.len()),
            authorized_paths: Vec::new(),
            restriction_roots: BTreeMap::new(),
        },
        |mut acc, (path, infos)| {
            let restricted = !infos.is_empty();
            acc.is_restricted.push(restricted);

            let access = if restricted {
                infos.iter().all(|info| info.has_access.unwrap_or(false))
            } else {
                true
            };
            acc.has_access.push(access);

            if check_permissions && access {
                acc.authorized_paths.push(path.to_string());
            }

            if restricted {
                acc.restriction_roots.insert(
                    path.to_string(),
                    infos
                        .iter()
                        .map(|info| thrift::PathRestrictionRoot {
                            path: info.restriction_root().to_string(),
                            acls: vec![info.repo_region_acl().to_string()],
                            ..Default::default()
                        })
                        .collect(),
                );
            }

            acc
        },
    );

    let authorized_paths = {
        let mut paths = result.authorized_paths;
        paths.sort();
        paths
    };

    Ok(thrift::CommitRestrictedPathsAccessResponse {
        are_restricted: compute_path_coverage(result.is_restricted),
        has_access: compute_path_coverage(result.has_access),
        restriction_roots: result.restriction_roots,
        authorized_paths,
        ..Default::default()
    })
}

/// Find all restriction roots that are nested under the given filter paths.
/// Returns a stream of restriction roots where the restriction root path
/// starts with one of the filter paths (i.e., the root is under the filter).
///
/// If filter_roots is empty, returns all restriction roots in the repository.
pub(crate) async fn find_nested_restricted_roots(
    cs_ctx: &ChangesetContext<Repo>,
    filter_roots: BTreeSet<String>,
) -> Result<
    BoxStream<
        'static,
        Result<thrift::CommitFindRestrictedPathsStreamItem, scs_errors::ServiceError>,
    >,
    scs_errors::ServiceError,
> {
    let roots: Vec<MPath> = if filter_roots.is_empty() {
        vec![MPath::ROOT]
    } else {
        filter_roots
            .iter()
            .map(|r| {
                MPath::try_from(r.as_str())
                    .map_err(|e| MononokeError::InvalidRequest(e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    let descendants = cs_ctx.find_restricted_descendants(roots).await?;

    Ok((async_stream::stream! {
        for info in descendants {
            yield Ok(thrift::CommitFindRestrictedPathsStreamItem {
                path: info.restriction_root().to_string(),
                acls: vec![info.repo_region_acl().to_string()],
                ..Default::default()
            });
        }
    })
    .boxed())
}

/// Check if the mock API should be used for this repo.
pub(crate) fn use_mock_api(repo_name: &str) -> Result<bool, scs_errors::ServiceError> {
    justknobs::eval(
        "scm/mononoke:scs_restricted_paths_use_mock_api",
        None,
        Some(repo_name),
    )
    .map_err(|e| {
        scs_errors::internal_error(format!(
            "Failed to read JustKnob scm/mononoke:scs_restricted_paths_use_mock_api: {e}"
        ))
        .into()
    })
}

pub(crate) fn compute_path_coverage(
    values: impl IntoIterator<Item = bool>,
) -> thrift::PathCoverage {
    let (has_true, has_false) = values.into_iter().fold((false, false), |(ht, hf), value| {
        (ht || value, hf || !value)
    });

    match (has_true, has_false) {
        (true, true) => thrift::PathCoverage::SOME,
        (true, false) => thrift::PathCoverage::ALL,
        (false, _) => thrift::PathCoverage::NONE,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;

    use anyhow::Context;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use metaconfig_types::RestrictedPathsConfig;
    use mononoke_api::Repo;
    use mononoke_api::RepoContext;
    use mononoke_api::changeset::ChangesetContext;
    use mononoke_macros::mononoke;
    use mononoke_types::RepositoryId;
    use permission_checker::MononokeIdentity;
    use permission_checker::dummy::DummyAclProvider;
    use repo_derived_data::RepoDerivedDataArc;
    use restricted_paths::ArcRestrictedPaths;
    use restricted_paths::RestrictedPaths;
    use restricted_paths::RestrictedPathsConfigBased;
    use restricted_paths::SqlRestrictedPathsManifestIdStoreBuilder;
    use scuba_ext::MononokeScubaSampleBuilder;
    use sql_construct::SqlConstruct;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::CreateCommitContext;

    use super::*;

    /// Helper to create RestrictedPaths for testing
    async fn create_test_restricted_paths(
        fb: FacebookInit,
        path_acls: Vec<(&str, &str)>, // (path, "TYPE:acl_name")
    ) -> Result<ArcRestrictedPaths> {
        let repo_id = RepositoryId::new(0);

        let path_acls_map: HashMap<NonRootMPath, MononokeIdentity> = path_acls
            .into_iter()
            .map(|(path, acl_str)| {
                Ok((
                    NonRootMPath::new(path)
                        .context("Failed to create NonRootMPath from test path")?,
                    MononokeIdentity::from_str(acl_str)
                        .context("Failed to parse MononokeIdentity from ACL string")?,
                ))
            })
            .collect::<Result<HashMap<_, _>>>()?;

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

        // TODO(T248649079): test the ACL checks logic
        let acl_provider = DummyAclProvider::new(fb)?;
        let scuba = MononokeScubaSampleBuilder::with_discard();

        let config_based = Arc::new(RestrictedPathsConfigBased::new(
            config,
            manifest_id_store,
            None,
        ));

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

    /// Helper to create a ChangesetContext with restricted paths for testing.
    async fn create_test_changeset(
        fb: FacebookInit,
        path_acls: Vec<(&str, &str)>,
    ) -> Result<ChangesetContext<Repo>> {
        let restricted_paths = create_test_restricted_paths(fb, path_acls).await?;
        let ctx = CoreContext::test_mock(fb);

        let repo: Repo = TestRepoFactory::new(fb)
            .context("Failed to create TestRepoFactory")?
            .with_restricted_paths(restricted_paths)
            .build()
            .await
            .context("Failed to build test repo")?;

        let root_cs_id = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "content")
            .commit()
            .await
            .context("Failed to create root commit")?;

        let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo))
            .await
            .context("Failed to create test RepoContext")?;

        let cs_ctx = repo_ctx
            .changeset(root_cs_id)
            .await
            .context(format!("Failed to resolve changeset {root_cs_id}"))?
            .ok_or_else(|| anyhow::anyhow!("Changeset {root_cs_id} not found"))?;

        Ok(cs_ctx)
    }

    // Tests for `restricted_paths_access_impl`

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_access_exact_match(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(fb, vec![("restricted/dir", "TIER:my-acl")]).await?;

        let paths = BTreeSet::from(["restricted/dir".to_string()]);
        let response = restricted_paths_access_impl(&cs_ctx, paths, false)
            .await
            .expect("restricted_paths_access_impl failed");

        assert_eq!(response.are_restricted, thrift::PathCoverage::ALL);
        assert_eq!(response.restriction_roots.len(), 1);
        let roots = response
            .restriction_roots
            .get("restricted/dir")
            .expect("Expected restriction root for 'restricted/dir'");
        assert_eq!(roots[0].path, "restricted/dir");
        assert_eq!(roots[0].acls, vec!["TIER:my-acl"]);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_access_nested_path(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(fb, vec![("restricted", "TIER:my-acl")]).await?;

        let paths = BTreeSet::from(["restricted/subdir/file.txt".to_string()]);
        let response = restricted_paths_access_impl(&cs_ctx, paths, false)
            .await
            .expect("restricted_paths_access_impl failed");

        assert_eq!(response.are_restricted, thrift::PathCoverage::ALL);
        assert_eq!(response.restriction_roots.len(), 1);
        let roots = response
            .restriction_roots
            .get("restricted/subdir/file.txt")
            .expect("Expected restriction root for 'restricted/subdir/file.txt'");
        assert_eq!(roots[0].path, "restricted");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_access_no_match(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(fb, vec![("restricted/dir", "TIER:my-acl")]).await?;

        let paths = BTreeSet::from(["other/path/file.txt".to_string()]);
        let response = restricted_paths_access_impl(&cs_ctx, paths, false)
            .await
            .expect("restricted_paths_access_impl failed");

        assert_eq!(response.are_restricted, thrift::PathCoverage::NONE);
        assert!(response.restriction_roots.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_access_sibling_path(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(fb, vec![("foo/bar", "TIER:my-acl")]).await?;

        // foo/baz is a sibling of foo/bar, not under it
        let paths = BTreeSet::from(["foo/baz/file.txt".to_string()]);
        let response = restricted_paths_access_impl(&cs_ctx, paths, false)
            .await
            .expect("restricted_paths_access_impl failed");

        assert_eq!(response.are_restricted, thrift::PathCoverage::NONE);
        assert!(response.restriction_roots.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_access_multiple_restrictions_single_path(
        fb: FacebookInit,
    ) -> Result<()> {
        let cs_ctx = create_test_changeset(
            fb,
            vec![("first", "TIER:first-acl"), ("second", "TIER:second-acl")],
        )
        .await?;

        let paths = BTreeSet::from(["second/nested/file.txt".to_string()]);
        let response = restricted_paths_access_impl(&cs_ctx, paths, false)
            .await
            .expect("restricted_paths_access_impl failed");

        assert_eq!(response.are_restricted, thrift::PathCoverage::ALL);
        let roots = response
            .restriction_roots
            .get("second/nested/file.txt")
            .expect("Expected restriction root for 'second/nested/file.txt'");
        assert_eq!(roots[0].path, "second");
        assert_eq!(roots[0].acls, vec!["TIER:second-acl"]);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_access_multiple_paths_from_different_roots(
        fb: FacebookInit,
    ) -> Result<()> {
        let cs_ctx = create_test_changeset(
            fb,
            vec![
                ("first", "TIER:first-acl"),
                ("second", "TIER:second-acl"),
                ("third/nested", "TIER:third-acl"),
            ],
        )
        .await?;

        let paths = BTreeSet::from([
            "first/file1.txt".to_string(),
            "second/subdir/file2.txt".to_string(),
            "third/nested/deep/file3.txt".to_string(),
            "unrestricted/file4.txt".to_string(),
        ]);
        let response = restricted_paths_access_impl(&cs_ctx, paths, false)
            .await
            .expect("restricted_paths_access_impl failed");

        // 3 restricted + 1 unrestricted = SOME
        assert_eq!(response.are_restricted, thrift::PathCoverage::SOME);
        assert_eq!(response.restriction_roots.len(), 3);

        // Verify each restricted path has correct root
        let first_roots = response
            .restriction_roots
            .get("first/file1.txt")
            .expect("Expected restriction root for 'first/file1.txt'");
        assert_eq!(first_roots[0].path, "first");
        assert_eq!(first_roots[0].acls, vec!["TIER:first-acl"]);

        let second_roots = response
            .restriction_roots
            .get("second/subdir/file2.txt")
            .expect("Expected restriction root for 'second/subdir/file2.txt'");
        assert_eq!(second_roots[0].path, "second");
        assert_eq!(second_roots[0].acls, vec!["TIER:second-acl"]);

        let third_roots = response
            .restriction_roots
            .get("third/nested/deep/file3.txt")
            .expect("Expected restriction root for 'third/nested/deep/file3.txt'");
        assert_eq!(third_roots[0].path, "third/nested");
        assert_eq!(third_roots[0].acls, vec!["TIER:third-acl"]);

        // Unrestricted path should not be in restriction_roots
        assert!(
            !response
                .restriction_roots
                .contains_key("unrestricted/file4.txt")
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_access_no_restrictions_configured(
        fb: FacebookInit,
    ) -> Result<()> {
        let cs_ctx = create_test_changeset(fb, vec![]).await?;

        let paths = BTreeSet::from(["any/path/file.txt".to_string()]);
        let response = restricted_paths_access_impl(&cs_ctx, paths, false)
            .await
            .expect("restricted_paths_access_impl failed");

        assert_eq!(response.are_restricted, thrift::PathCoverage::NONE);
        assert_eq!(response.has_access, thrift::PathCoverage::ALL);
        assert!(response.restriction_roots.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_access_check_permissions_populates_authorized_paths(
        fb: FacebookInit,
    ) -> Result<()> {
        let cs_ctx = create_test_changeset(fb, vec![("restricted", "TIER:my-acl")]).await?;

        let paths = BTreeSet::from([
            "restricted/file.txt".to_string(),
            "unrestricted/file.txt".to_string(),
        ]);
        let response = restricted_paths_access_impl(
            &cs_ctx, paths, true, // check_permissions = true
        )
        .await
        .expect("restricted_paths_access_impl failed");

        // DummyAclProvider grants access, so both paths should be authorized
        // The unrestricted path is always authorized, and restricted path
        // depends on DummyAclProvider behavior
        assert!(!response.authorized_paths.is_empty());
        assert!(
            response
                .authorized_paths
                .contains(&"unrestricted/file.txt".to_string())
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_access_all_paths_restricted(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(fb, vec![("restricted", "TIER:my-acl")]).await?;

        let paths = BTreeSet::from([
            "restricted/file1.txt".to_string(),
            "restricted/file2.txt".to_string(),
            "restricted/subdir/file3.txt".to_string(),
        ]);
        let response = restricted_paths_access_impl(&cs_ctx, paths, false)
            .await
            .expect("restricted_paths_access_impl failed");

        assert_eq!(response.are_restricted, thrift::PathCoverage::ALL);
        assert_eq!(response.restriction_roots.len(), 3);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_restricted_paths_access_all_paths_unrestricted(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(fb, vec![("restricted", "TIER:my-acl")]).await?;

        let paths = BTreeSet::from([
            "unrestricted/file1.txt".to_string(),
            "other/file2.txt".to_string(),
            "another/subdir/file3.txt".to_string(),
        ]);
        let response = restricted_paths_access_impl(&cs_ctx, paths, false)
            .await
            .expect("restricted_paths_access_impl failed");

        assert_eq!(response.are_restricted, thrift::PathCoverage::NONE);
        assert!(response.restriction_roots.is_empty());

        Ok(())
    }

    // Tests for `find_nested_restricted_roots`

    /// Helper to collect stream results into a Vec for easier testing
    async fn collect_nested_roots(
        cs_ctx: &ChangesetContext<Repo>,
        filter: BTreeSet<String>,
    ) -> Result<Vec<(String, String)>> {
        use futures::TryStreamExt;
        use scs_errors::LoggableError;

        find_nested_restricted_roots(cs_ctx, filter)
            .await
            .map_err(|e| anyhow::anyhow!(e.status_and_description().1))?
            .map_ok(|item| (item.path, item.acls.into_iter().next().unwrap_or_default()))
            .try_collect()
            .await
            .map_err(|e| anyhow::anyhow!(e.status_and_description().1))
            .context("Collecting stream into vec")
    }

    #[mononoke::fbinit_test]
    async fn test_find_nested_roots_no_restrictions(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(fb, vec![]).await?;
        let filter = BTreeSet::new();

        let result = collect_nested_roots(&cs_ctx, filter).await?;

        assert!(result.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_nested_roots_empty_filter_returns_all(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(
            fb,
            vec![
                ("first/path", "TIER:first-acl"),
                ("second/path", "TIER:second-acl"),
            ],
        )
        .await?;
        let filter = BTreeSet::new();

        let result = collect_nested_roots(&cs_ctx, filter).await?;

        assert_eq!(result.len(), 2);
        // Verify both paths are present (order may vary due to HashMap iteration)
        let paths: Vec<&str> = result.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"first/path"));
        assert!(paths.contains(&"second/path"));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_nested_roots_filter_exact_match(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(
            fb,
            vec![
                ("first/path", "TIER:first-acl"),
                ("second/path", "TIER:second-acl"),
            ],
        )
        .await?;
        let filter = BTreeSet::from(["first/path".to_string()]);

        let result = collect_nested_roots(&cs_ctx, filter).await?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "first/path");
        assert_eq!(result[0].1, "TIER:first-acl");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_nested_roots_filter_parent_of_root(fb: FacebookInit) -> Result<()> {
        // Filter is parent of root (root starts with filter)
        let cs_ctx = create_test_changeset(fb, vec![("foo/bar/restricted", "TIER:my-acl")]).await?;
        let filter = BTreeSet::from(["foo".to_string()]);

        let result = collect_nested_roots(&cs_ctx, filter).await?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "foo/bar/restricted");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_nested_roots_filter_child_of_root_returns_empty(
        fb: FacebookInit,
    ) -> Result<()> {
        // Filter is a child of the root - should NOT match because we only return
        // roots that are under the filter, not roots that contain the filter
        let cs_ctx = create_test_changeset(fb, vec![("foo/bar", "TIER:my-acl")]).await?;
        let filter = BTreeSet::from(["foo/bar/baz/deep".to_string()]);

        let result = collect_nested_roots(&cs_ctx, filter).await?;

        // The root "foo/bar" contains the filter "foo/bar/baz/deep", but is not under it,
        // so it should NOT be returned
        assert!(result.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_nested_roots_filter_no_match(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(
            fb,
            vec![
                ("first/path", "TIER:first-acl"),
                ("second/path", "TIER:second-acl"),
            ],
        )
        .await?;
        let filter = BTreeSet::from(["third/path".to_string()]);

        let result = collect_nested_roots(&cs_ctx, filter).await?;

        assert!(result.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_nested_roots_filter_sibling_no_match(fb: FacebookInit) -> Result<()> {
        // Sibling paths should not match
        let cs_ctx = create_test_changeset(fb, vec![("foo/bar", "TIER:my-acl")]).await?;
        let filter = BTreeSet::from(["foo/baz".to_string()]);

        let result = collect_nested_roots(&cs_ctx, filter).await?;

        assert!(result.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_nested_roots_multiple_filters(fb: FacebookInit) -> Result<()> {
        let cs_ctx = create_test_changeset(
            fb,
            vec![
                ("first/path", "TIER:first-acl"),
                ("second/path", "TIER:second-acl"),
                ("third/path", "TIER:third-acl"),
            ],
        )
        .await?;
        let filter = BTreeSet::from(["first/path".to_string(), "third/path".to_string()]);

        let result = collect_nested_roots(&cs_ctx, filter).await?;

        assert_eq!(result.len(), 2);
        let paths: Vec<&str> = result.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"first/path"));
        assert!(paths.contains(&"third/path"));
        assert!(!paths.contains(&"second/path"));

        Ok(())
    }

    // Other unit tests

    #[mononoke::test]
    fn test_compute_path_coverage_empty() {
        assert_eq!(compute_path_coverage(vec![]), thrift::PathCoverage::NONE);
    }

    #[mononoke::test]
    fn test_compute_path_coverage_all_true() {
        assert_eq!(
            compute_path_coverage(vec![true, true, true]),
            thrift::PathCoverage::ALL
        );
    }

    #[mononoke::test]
    fn test_compute_path_coverage_all_false() {
        assert_eq!(
            compute_path_coverage(vec![false, false]),
            thrift::PathCoverage::NONE
        );
    }

    #[mononoke::test]
    fn test_compute_path_coverage_mixed() {
        assert_eq!(
            compute_path_coverage(vec![true, false, true]),
            thrift::PathCoverage::SOME
        );
    }

    #[mononoke::test]
    fn test_is_prefix_of_same_path() {
        let path = NonRootMPath::new("foo/bar").expect("Failed to create NonRootMPath");
        assert!(path.is_prefix_of(&path));
    }

    #[mononoke::test]
    fn test_compute_path_coverage_single_true() {
        assert_eq!(compute_path_coverage(vec![true]), thrift::PathCoverage::ALL);
    }

    #[mononoke::test]
    fn test_compute_path_coverage_single_false() {
        assert_eq!(
            compute_path_coverage(vec![false]),
            thrift::PathCoverage::NONE
        );
    }
}
