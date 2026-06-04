/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

use configmodel::Config;
use configmodel::ConfigExt;

pub struct PermissionDeniedResult {
    pub warnings: Vec<String>,
    pub exit_nonzero: bool,
}

pub fn check_permission_denied_paths(
    paths: &context::PermissionDeniedPaths,
    config: &Arc<dyn Config>,
) -> anyhow::Result<PermissionDeniedResult> {
    let denied = paths.lock();
    if denied.is_empty() {
        return Ok(PermissionDeniedResult {
            warnings: Vec::new(),
            exit_nonzero: false,
        });
    }

    let mode = config.get_or("slacl", "on-permission-denied", || "error".to_string())?;
    if mode == "ignore" {
        return Ok(PermissionDeniedResult {
            warnings: Vec::new(),
            exit_nonzero: false,
        });
    }

    let url_template = config
        .get_or("slacl", "request-access-url-template", String::new)
        .unwrap_or_default();

    // Group unique paths by ACL name.
    let mut by_acl: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for err in denied.iter() {
        let acl = if err.request_acl.is_empty() {
            "(unknown)".to_string()
        } else {
            err.request_acl.clone()
        };
        by_acl.entry(acl).or_default().insert(err.path.to_string());
    }

    let warnings = format_warnings(&by_acl, &url_template);

    Ok(PermissionDeniedResult {
        exit_nonzero: mode == "error",
        warnings,
    })
}

fn format_warnings(by_acl: &BTreeMap<String, BTreeSet<String>>, url_template: &str) -> Vec<String> {
    let mut lines = vec!["warning: results may be incomplete due to path ACLs\n".to_string()];

    for (acl, paths) in by_acl {
        let mut paths_iter = paths.iter();
        let first = match paths_iter.next() {
            Some(p) => p,
            None => continue,
        };

        let mut line = format!("  '{first}'");
        let remaining = paths.len() - 1;
        if remaining > 0 {
            line.push_str(&format!(" [and {remaining} more]"));
        }

        if remaining > 0 {
            line.push_str(" are restricted");
        } else {
            line.push_str(" is restricted");
        }

        if !url_template.is_empty() {
            let url = url_template.replace("{acl}", acl);
            line.push_str(&format!(" by ACL '{acl}' - request access at {url}"));
        } else {
            line.push_str(&format!(" by ACL '{acl}'"));
        }

        line.push('\n');
        lines.push(line);
    }

    lines
}

/// Format a single PermissionDenied error for user-facing display.
/// Used both for "direct" errors (command aborts) and "indirect" warnings.
pub fn format_permission_denied_error(
    err: &types::errors::PermissionDenied,
    config: &dyn Config,
) -> String {
    let url_template = config
        .get_or("slacl", "request-access-url-template", String::new)
        .unwrap_or_default();

    let mut msg = format!("path '{}' is restricted", err.path);
    if !err.request_acl.is_empty() {
        if !url_template.is_empty() {
            let url = url_template.replace("{acl}", &err.request_acl);
            msg.push_str(&format!(
                " by ACL '{}' - request access at {}",
                err.request_acl, url
            ));
        } else {
            msg.push_str(&format!(" by ACL '{}'", err.request_acl));
        }
    }
    msg
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;

    use super::*;

    fn make_by_acl(entries: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
        let mut by_acl = BTreeMap::new();
        for (acl, paths) in entries {
            let set: BTreeSet<String> = paths.iter().map(|p| p.to_string()).collect();
            by_acl.insert(acl.to_string(), set);
        }
        by_acl
    }

    #[test]
    fn test_single_path_single_acl() {
        let by_acl = make_by_acl(&[("my-acl", &["secret/dir"])]);
        let warnings = format_warnings(&by_acl, "");
        assert_eq!(
            warnings,
            vec![
                "warning: results may be incomplete due to path ACLs\n",
                "  'secret/dir' is restricted by ACL 'my-acl'\n",
            ]
        );
    }

    #[test]
    fn test_multiple_paths_single_acl() {
        let by_acl = make_by_acl(&[("my-acl", &["a/dir", "b/dir", "c/dir"])]);
        let warnings = format_warnings(&by_acl, "");
        assert_eq!(
            warnings,
            vec![
                "warning: results may be incomplete due to path ACLs\n",
                "  'a/dir' [and 2 more] are restricted by ACL 'my-acl'\n",
            ]
        );
    }

    #[test]
    fn test_multiple_acls() {
        let by_acl = make_by_acl(&[("acl-a", &["dir1"]), ("acl-b", &["dir2", "dir3"])]);
        let warnings = format_warnings(&by_acl, "");
        assert_eq!(
            warnings,
            vec![
                "warning: results may be incomplete due to path ACLs\n",
                "  'dir1' is restricted by ACL 'acl-a'\n",
                "  'dir2' [and 1 more] are restricted by ACL 'acl-b'\n",
            ]
        );
    }

    #[test]
    fn test_with_url_template() {
        let by_acl = make_by_acl(&[("my-acl", &["secret"])]);
        let warnings = format_warnings(&by_acl, "https://access.example.com/request?acl={acl}");
        assert_eq!(
            warnings,
            vec![
                "warning: results may be incomplete due to path ACLs\n",
                "  'secret' is restricted by ACL 'my-acl' - request access at https://access.example.com/request?acl=my-acl\n",
            ]
        );
    }

    #[test]
    fn test_dedup_paths() {
        let mut by_acl = BTreeMap::new();
        let mut paths = BTreeSet::new();
        paths.insert("same/dir".to_string());
        paths.insert("same/dir".to_string()); // BTreeSet deduplicates
        by_acl.insert("acl".to_string(), paths);
        let warnings = format_warnings(&by_acl, "");
        assert_eq!(
            warnings,
            vec![
                "warning: results may be incomplete due to path ACLs\n",
                "  'same/dir' is restricted by ACL 'acl'\n",
            ]
        );
    }

    #[test]
    fn test_format_permission_denied_error_basic() {
        let err = types::errors::PermissionDenied {
            path: "secret/dir".to_string().try_into().unwrap(),
            hgid: types::HgId::null_id().clone(),
            request_acl: "my-acl".to_string(),
        };
        let config = configset::ConfigSet::new();
        let msg = format_permission_denied_error(&err, &config);
        assert_eq!(msg, "path 'secret/dir' is restricted by ACL 'my-acl'");
    }

    #[test]
    fn test_format_permission_denied_error_empty_acl() {
        let err = types::errors::PermissionDenied {
            path: "secret/dir".to_string().try_into().unwrap(),
            hgid: types::HgId::null_id().clone(),
            request_acl: String::new(),
        };
        let config = configset::ConfigSet::new();
        let msg = format_permission_denied_error(&err, &config);
        assert_eq!(msg, "path 'secret/dir' is restricted");
    }

    #[test]
    fn test_format_permission_denied_error_with_url() {
        let err = types::errors::PermissionDenied {
            path: "secret/dir".to_string().try_into().unwrap(),
            hgid: types::HgId::null_id().clone(),
            request_acl: "my-acl".to_string(),
        };
        let mut config = configset::ConfigSet::new();
        config.set(
            "slacl",
            "request-access-url-template",
            Some("https://access.example.com/?acl={acl}"),
            &Default::default(),
        );
        let msg = format_permission_denied_error(&err, &config);
        assert_eq!(
            msg,
            "path 'secret/dir' is restricted by ACL 'my-acl' - request access at https://access.example.com/?acl=my-acl"
        );
    }
}
