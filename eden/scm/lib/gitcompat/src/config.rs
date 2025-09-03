/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;

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
                &[
                    "--show-scope",
                    "--get-regexp",
                    "^(remote|user|submodule)\\.",
                ],
            )
            .output()?;
        let out = String::from_utf8(out.stdout)?;
        Ok(translate_git_config_output(&out))
    }
}

#[derive(Copy, Default, Clone)]
struct SubmoduleActiveness {
    has_url: bool,
    explicit_active: Option<bool>,
}

impl SubmoduleActiveness {
    fn is_active(&self) -> bool {
        match (self.has_url, self.explicit_active) {
            (_, Some(v)) => v,
            (v, None) => v,
        }
    }
}

/// Translate git config to `(user_config, repo_config)`.
fn translate_git_config_output(out: &str) -> (String, String) {
    // Example output (actually separated by a tab, not spaces):
    //  global  user.name Foo Bar
    //  global  user.email foo@example.com
    //  local   remote.origin.url https://example.com/foo/bar
    //  local   remote.origin.fetch +refs/heads/*:refs/remotes/origin/*
    //  local   remote.origin.pushurl git@example.com/foo/bar
    //  local   user.email foo@example.net
    //  local   submodule.active .
    //  local   submodule.sub.url submodule-url

    let mut global_user = "";
    let mut global_email = "";
    let mut local_user = "";
    let mut local_email = "";
    let mut paths_config = Vec::new();

    // submodule.active config (default off)
    let mut submodule_global_active = false;

    // individual submodules
    let mut submodule_individual_active = BTreeMap::<&str, SubmoduleActiveness>::new();

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
                    } else if let Some(rest_name) = name.strip_prefix("submodule.") {
                        // NOTE: Simplified handling, not fully confront to Git's spec [1].
                        // Git allows `submodule.active` to be a "pathspec". Practically it is
                        // usually set to ".".
                        // [1]: https://git-scm.com/docs/gitsubmodules
                        if rest_name == "active" {
                            submodule_global_active = value == ".";
                        } else if let Some((submodule_name, config_name)) =
                            rest_name.split_once('.')
                        {
                            match config_name {
                                "url" => {
                                    submodule_individual_active
                                        .entry(submodule_name)
                                        .or_default()
                                        .has_url = true
                                }
                                "active" => {
                                    submodule_individual_active
                                        .entry(submodule_name)
                                        .or_default()
                                        .explicit_active = Some(value == "true")
                                }
                                _ => {}
                            }
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

    // Submodules
    repo_config.push_str("[submodule]\nactive = ");
    repo_config.push_str(bool_str(submodule_global_active));
    repo_config.push('\n');
    for (name, activeness) in submodule_individual_active {
        repo_config.push_str("active-");
        repo_config.push_str(name);
        repo_config.push_str(" = ");
        repo_config.push_str(bool_str(activeness.is_active()));
        repo_config.push('\n');
    }

    (user_config, repo_config)
}

fn str_or<'a>(lhs: &'a str, rhs: &'a str) -> &'a str {
    if lhs.is_empty() { rhs } else { lhs }
}

fn bool_str(v: bool) -> &'static str {
    match v {
        true => "true",
        false => "false",
    }
}

fn normalize_remote_name(name: &str) -> &str {
    if name == "origin" { "default" } else { name }
}

/// translate "a@b:c" to "ssh://a@b/c".
fn translate_scp_url_to_ssh(value: &str) -> Cow<'_, str> {
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
            if cfg!(windows)
                && left.len() == 1
                && (right.starts_with('/') || right.starts_with('\\'))
            {
                // Likely Windows path, like C:\foo\bar.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_user_config_to_sapling_config() {
        let out = r#"global	user.name Foo Bar
global	user.email foorbar@example.com
local	remote.origin.url https://example.com/foo/repo
local	remote.origin.pushurl git@example.com:foo/repo
local	remote.upstream.url https://example.com/upstream/repo
local	user.email foo@bar.net
        "#;
        let (user, repo) = translate_git_config_output(out);
        assert_eq!(
            user,
            r#"[ui]
# from git config: user.name and user.email
username = Foo Bar <foorbar@example.com>
"#
        );
        assert_eq!(
            repo,
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
[submodule]
active = false
"#
        );
    }

    #[test]
    fn test_git_submodule_config_to_sapling_config() {
        let out = r#"
local	submodule.active .
local	submodule.sub.url sub1
"#;
        let (user, repo) = translate_git_config_output(out);
        assert_eq!(user, "");
        assert_eq!(repo, "[submodule]\nactive = true\nactive-sub = true\n");

        let out = r#"
local	submodule.sub/1.url sub1
local	submodule.sub/1.active false
local	submodule.sub/2.active true
"#;
        let (user, repo) = translate_git_config_output(out);
        assert_eq!(user, "");
        assert_eq!(
            repo,
            "[submodule]\nactive = false\nactive-sub/1 = false\nactive-sub/2 = true\n"
        );
    }

    #[test]
    fn test_translate_scp_url_to_ssh() {
        assert_eq!(translate_scp_url_to_ssh("a:b"), "ssh://a/b");
        assert_eq!(translate_scp_url_to_ssh("a@b.com:c/d"), "ssh://a@b.com/c/d");
        assert_eq!(translate_scp_url_to_ssh("./a:b"), "./a:b");
    }

    #[cfg(windows)]
    #[test]
    fn test_translate_scp_url_to_ssh_windows() {
        assert_eq!(translate_scp_url_to_ssh("C:\\foo\\bar"), "C:\\foo\\bar");
        assert_eq!(translate_scp_url_to_ssh("X:/bar"), "X:/bar");
    }
}
