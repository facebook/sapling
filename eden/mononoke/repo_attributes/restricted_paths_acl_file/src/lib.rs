/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Parser for restricted paths ACL files (.slacl)

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use metaconfig_types::RestrictedPathsAclFile;
use metaconfig_types::parse_bare_group_name;
use mononoke_types::FileContents;
use permission_checker::MononokeIdentity;
use repos::RawRestrictedPathsAclFile;

const SUPPORTED_VERSION: i32 = 0;

/// Parse ACL file content (TOML-serialized)
pub fn parse_acl_file(content: &FileContents) -> Result<RestrictedPathsAclFile> {
    let bytes = match content {
        FileContents::Bytes(bytes) => bytes,
        FileContents::Chunked(_) => bail!("ACL files shouldn't be chunked"),
    };
    let content_str = std::str::from_utf8(bytes)?;
    let raw: RawRestrictedPathsAclFile = toml::from_str(content_str)?;

    let version = raw.version.unwrap_or(SUPPORTED_VERSION);
    if version != SUPPORTED_VERSION {
        bail!("Unsupported ACL file version: {version} (expected {SUPPORTED_VERSION})",);
    }

    let repo_region_acl: MononokeIdentity = raw.repo_region_acl.parse()?;
    let permission_request_group = raw
        .permission_request_group
        .map(|s| s.parse())
        .transpose()?;
    let rollout_allowlist_group = raw
        .rollout_allowlist_group
        .as_deref()
        .map(parse_bare_group_name)
        .transpose()
        .context("Invalid rollout_allowlist_group")?;

    let acl_file = RestrictedPathsAclFile::new(
        repo_region_acl,
        permission_request_group,
        rollout_allowlist_group,
    )?;
    Ok(acl_file)
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    fn file_contents(bytes: &'static [u8]) -> FileContents {
        FileContents::new_bytes(bytes)
    }

    #[mononoke::test]
    fn test_parse_valid_acl_file() {
        let content =
            file_contents(b"repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n");
        let result = parse_acl_file(&content).unwrap();
        assert_eq!(
            result.repo_region_acl(),
            &MononokeIdentity::from_legacy_type_data("REPO_REGION", "repos/hg/fbsource/=project1"),
        );
        assert_eq!(result.permission_request_group(), None);
    }

    #[mononoke::test]
    fn test_parse_with_permission_request_group() {
        let content = file_contents(
            br#"
repo_region_acl = "REPO_REGION:repos/hg/fbsource/=project1"
permission_request_group = "GROUP:some_amp_group"
"#,
        );
        let result = parse_acl_file(&content).unwrap();
        assert_eq!(
            result.repo_region_acl(),
            &MononokeIdentity::from_legacy_type_data("REPO_REGION", "repos/hg/fbsource/=project1"),
        );
        assert_eq!(
            result.permission_request_group(),
            Some(MononokeIdentity::from_legacy_type_data(
                "GROUP",
                "some_amp_group"
            ))
            .as_ref(),
        );
    }

    #[mononoke::test]
    fn test_parse_with_explicit_version_zero() {
        let content = file_contents(
            br#"
version = 0
repo_region_acl = "REPO_REGION:repos/hg/fbsource/=project1"
"#,
        );
        let result = parse_acl_file(&content).unwrap();
        assert_eq!(
            result.repo_region_acl(),
            &MononokeIdentity::from_legacy_type_data("REPO_REGION", "repos/hg/fbsource/=project1"),
        );
    }

    #[mononoke::test]
    fn test_parse_unsupported_version_fails() {
        let content = file_contents(
            br#"
version = 1
repo_region_acl = "REPO_REGION:repos/hg/fbsource/=project1"
"#,
        );
        let result = parse_acl_file(&content);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unsupported ACL file version")
        );
    }

    #[mononoke::test]
    fn test_parse_invalid_toml_fails() {
        let content = file_contents(b"not valid toml [[[");
        let result = parse_acl_file(&content);
        assert!(result.is_err());
    }

    #[mononoke::test]
    fn test_parse_missing_repo_region_acl_fails() {
        let content = file_contents(b"version = 0\n");
        let result = parse_acl_file(&content);
        assert!(result.is_err());
    }

    #[mononoke::test]
    fn test_parse_invalid_identity_format_fails() {
        let content = file_contents(b"repo_region_acl = \"missing_colon_separator\"\n");
        let result = parse_acl_file(&content);
        assert!(result.is_err());
    }

    /// What it tests: a bare `rollout_allowlist_group` in a `.slacl` file.
    /// Expected: it is prefixed to a `GROUP:` identity, the same spelling the
    /// config-backed source uses.
    #[mononoke::test]
    fn test_parse_with_rollout_allowlist_group() {
        let content = file_contents(
            br#"
repo_region_acl = "REPO_REGION:repos/hg/fbsource/=project1"
rollout_allowlist_group = "project1_rollout"
"#,
        );
        let result = parse_acl_file(&content).unwrap();
        assert_eq!(
            result.rollout_allowlist_group(),
            Some(MononokeIdentity::from_legacy_type_data(
                "GROUP",
                "project1_rollout"
            ))
            .as_ref(),
        );
    }

    /// What it tests: a `.slacl` file that omits `rollout_allowlist_group`.
    /// Expected: no rollout allowlist, so callers are never allowlisted for it.
    #[mononoke::test]
    fn test_parse_without_rollout_allowlist_group() {
        let content =
            file_contents(b"repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n");
        let result = parse_acl_file(&content).unwrap();
        assert_eq!(result.rollout_allowlist_group(), None);
    }

    /// What it tests: malformed `rollout_allowlist_group` values in a `.slacl`.
    /// Expected: each is rejected, so a tent owner cannot end up with an
    /// allowlist that silently never matches anyone.
    #[mononoke::test]
    fn test_parse_malformed_rollout_allowlist_group_fails() {
        // (value, fragment the error must mention)
        let rejected = [
            ("", "must not be empty"),
            ("  ", "whitespace"),
            (" project1_rollout", "whitespace"),
            ("GROUP:project1_rollout", "omit the `GROUP:` prefix"),
        ];

        for (value, expected_fragment) in rejected {
            let content = file_contents_owned(format!(
                "repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n\
                 rollout_allowlist_group = \"{value}\"\n"
            ));
            let err = parse_acl_file(&content).expect_err(&format!("`{value}` should be rejected"));
            let msg = format!("{err:#}");
            assert!(
                msg.contains(expected_fragment),
                "error for `{value}` should mention {expected_fragment:?}, got: {msg}"
            );
        }
    }

    fn file_contents_owned(contents: String) -> FileContents {
        FileContents::new_bytes(contents.into_bytes())
    }
}
