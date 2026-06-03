/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Parser for restricted paths ACL files (.slacl)

use anyhow::Result;
use anyhow::bail;
use metaconfig_types::RestrictedPathsAclFile;
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

    let acl_file = RestrictedPathsAclFile::new(repo_region_acl, permission_request_group)?;
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
}
