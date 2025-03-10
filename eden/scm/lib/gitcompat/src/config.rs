/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use anyhow::Result;

use crate::rungit::GitCmd;
use crate::rungit::GlobalGit;

impl GlobalGit {
    /// Translate Git config to Sapling config. Includes remotes, and username.
    /// Return (system-user-level-config, repo-level-config).
    pub fn translate_git_config(&self) -> Result<(String, String)> {
        let out = self
            .git_cmd(
                "config",
                &["--show-scope", "--get-regexp", "^(remote|user)\\."],
            )
            .output()?;
        let config = String::from_utf8(out.stdout)?;
        let remotes_out = self
            .git_cmd(
                "ls-remote", &["--symref", ".", "HEAD"])
                .output()?;
        let remotes = String::from_utf8(remotes_out.stdout)?;
        Ok(translate_git_config_output(&config, &remotes))
    }
}

fn translate_git_config_output(out: &str, remotes: &str) -> (String, String) {
    // Example output:
    //  global  user.name Foo Bar
    //  global  user.email foo@example.com
    //  local   remote.origin.url https://example.com/foo/bar
    //  local   remote.origin.fetch +refs/heads/*:refs/remotes/origin/*
    //  local   remote.origin.pushurl git@example.com/foo/bar
    //  local   user.email foo@example.net

    let mut global_user = "";
    let mut global_email = "";
    let mut local_user = "";
    let mut local_email = "";
    let mut paths_config = Vec::new();

    for line in out.lines() {
        if let Some((scope, name, value)) = parse_git_config_output_line(line) {
            match (scope, name) {
                ("local", "user.name") => local_user = value,
                ("local", "user.email") => local_email = value,
                (_, "user.name") => global_user = value,
                (_, "user.email") => global_email = value,
                _ => {
                    if let Some(rest) = name.strip_prefix("remote.") {
                        if let Some(remote) = rest.strip_suffix(".url") {
                            paths_config.push(format!(
                                "# from git config: {}\n{} = {}\n",
                                name,
                                normalize_remote_name(remote),
                                translate_scp_url_to_ssh(value),
                            ));
                        } else if let Some(remote) = rest.strip_suffix(".pushurl") {
                            paths_config.push(format!(
                                "# from git config: {}\n{}-push = {}\n",
                                name,
                                normalize_remote_name(remote),
                                translate_scp_url_to_ssh(value),
                            ));
                        }
                    }
                }
            }
        }
    }

    let mut user_config = String::new();
    let mut repo_config = String::new();

    if !paths_config.is_empty() {
        repo_config.push_str(&format!("[paths]\n{}\n", paths_config.concat()));
    }

    if !global_user.is_empty() && !global_email.is_empty() {
        user_config.push_str(&format!(
            "[ui]\n# from git config: user.name and user.email\nusername = {} <{}>\n",
            global_user, global_email,
        ));
    }

    if !local_user.is_empty() || !local_email.is_empty() {
        repo_config.push_str(&format!(
            "[ui]\n# from git config: user.name and user.email\nusername = {} <{}>\n",
            str_or(local_user, global_user),
            str_or(local_email, global_email),
        ));
    }

    let default_publicheads = "origin/master,origin/main";
    if let Some(default_branch) = parse_symref_head(remotes) {
        repo_config.push_str(&format!(
            "\n[remotenames]\n# from git ls-remote\npublicheads=origin/{},{}\n",
            default_branch,
            default_publicheads,
        ));
    }


    (user_config, repo_config)
}

fn str_or<'a>(lhs: &'a str, rhs: &'a str) -> &'a str {
    if lhs.is_empty() { rhs } else { lhs }
}

fn normalize_remote_name(name: &str) -> &str {
    if name == "origin" { "default" } else { name }
}

/// translate "a@b:c" to "ssh://a@b/c".
fn translate_scp_url_to_ssh(value: &str) -> Cow<str> {
    // Check "man git-clone", "GIT URLS" for the specification.
    'not_scp: {
        if value.contains("://") {
            break 'not_scp;
        }

        if let Some((left, right)) = value.split_once(':') {
            if left.contains('/') {
                // "./foo:bar" is a filename.
                break 'not_scp;
            }
            let ssh_url = if let Some((user, host)) = left.split_once('@') {
                format!("ssh://{user}@{host}/{right}")
            } else {
                format!("ssh://{left}/{right}")
            };
            return Cow::Owned(ssh_url);
        }
    }
    Cow::Borrowed(value)
}

fn parse_git_config_output_line(line: &str) -> Option<(&str, &str, &str)> {
    let (scope, rest) = line.split_once('\t')?;
    let (name, value) = rest.split_once(' ')?;
    Some((scope, name, value))
}

fn parse_symref_head(lines: &str) -> Option<String> {
    for line in lines.lines() {
        // example: "ref: refs/heads/defaultbranch	HEAD"
        let prefix = "ref: refs/heads/";
        let suffix = "\tHEAD";
        if line.starts_with(prefix) && line.match_indices(suffix).count() ==1 {
            return Some(line[prefix.len()..line.len()-suffix.len()].to_string());
        }
    }
    return None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_config_to_sapling_config() {
        let out = r#"global	user.name Foo Bar
global	user.email foorbar@example.com
local	remote.origin.url https://example.com/foo/repo
local	remote.origin.pushurl git@example.com:foo/repo
local	remote.upstream.url https://example.com/upstream/repo
local	user.email foo@bar.net
        "#;
        let remotes = r#"
ref: refs/heads/defaultbranch	HEAD
4661e74b5ebe8727d1b0f8c29b1697f1f42daf70	HEAD
ref: refs/remotes/origin/defaultbranch	refs/remotes/origin/HEAD
4661e74b5ebe8727d1b0f8c29b1697f1f42daf70	refs/remotes/origin/HEAD
                "#;
        let (user, repo) = translate_git_config_output(out, remotes);
        assert_eq!(
            user,
            r#"[ui]
# from git config: user.name and user.email
username = Foo Bar <foorbar@example.com>
"#
        );
        let got = repo;
        let want = 
            r#"[paths]
# from git config: remote.origin.url
default = https://example.com/foo/repo
# from git config: remote.origin.pushurl
default-push = ssh://git@example.com/foo/repo
# from git config: remote.upstream.url
upstream = https://example.com/upstream/repo

[ui]
# from git config: user.name and user.email
username = Foo Bar <foo@bar.net>

[remotenames]
# from git ls-remote
publicheads=origin/defaultbranch,origin/master,origin/main
"#;
        assert_eq!(got, want, "\n expanded left: {got}\n------------\nexpanded right: {want}\n");
    }

    #[test]
    fn test_translate_scp_url_to_ssh() {
        assert_eq!(translate_scp_url_to_ssh("a:b"), "ssh://a/b");
        assert_eq!(translate_scp_url_to_ssh("a@b.com:c/d"), "ssh://a@b.com/c/d");
        assert_eq!(translate_scp_url_to_ssh("./a:b"), "./a:b");
    }
}
