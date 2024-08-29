/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use regex::Regex;

const WORKSPACE_NAME_PATTERN: &str = r"user/([^/]+)/.+";
const EMAIL_PATTERN: &str = r"^([a-zA-Z0-9_\-.]+)@([a-zA-Z0-9_\-.]+)\.([a-zA-Z]{2,5})$";
const LINUX_USER_PATTERN: &str = r"^[a-z_]([a-z0-9_-]{0,31}|[a-z0-9_-]{0,30}\\$)$";
const VALID_ACL_PATTERN: &str = "^[a-zA-Z0-9,=_\\.\\-]+([/][a-zA-Z0-9,=_\\.\\-]+)*$";

pub fn is_valid_workspace_structure(name: &str) -> (bool, Option<String>) {
    let validator =
        Regex::new(WORKSPACE_NAME_PATTERN).expect("Error while creating workspace regex");
    let owner = validator
        .captures(name)
        .and_then(|caps| caps.get(1).map(|match_| match_.as_str().to_string()));
    (validator.is_match(name), owner)
}
pub fn is_valid_email(email: &str) -> bool {
    let validator = Regex::new(EMAIL_PATTERN).expect("Error while creating email regex");
    validator.is_match(email)
}
pub fn is_valid_linux_user(user: &str) -> bool {
    let validator = Regex::new(LINUX_USER_PATTERN).expect("Error while creating linux user regex");
    validator.is_match(user)
}

pub fn sanity_check_workspace_name(name: &str) -> bool {
    let (valid, owner) = is_valid_workspace_structure(name);
    if let Some(owner) = owner {
        return valid && (is_valid_email(&owner) || is_valid_linux_user(&owner));
    }
    false
}

pub fn is_valid_acl_name(acl_name: &str) -> bool {
    let validator = Regex::new(VALID_ACL_PATTERN).expect("Error while creating email regex");
    validator.is_match(acl_name)
}

pub fn decorate_workspace_name_to_valid_acl_name(name: &str) -> String {
    let mut workspace_decorated = name.to_string();
    let allowed_punctuation = [',', '=', '_', '.', '-', '/'];
    workspace_decorated = workspace_decorated
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || allowed_punctuation.contains(&c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    workspace_decorated
}

pub fn make_workspace_acl_name(workspace: &str, reponame: &str) -> String {
    if is_valid_acl_name(workspace) {
        format!("{}/{}", reponame, workspace)
    } else {
        format!(
            "{}/{}",
            reponame,
            decorate_workspace_name_to_valid_acl_name(workspace)
        )
    }
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use crate::decorate_workspace_name_to_valid_acl_name;
    use crate::is_valid_acl_name;
    use crate::sanity_check_workspace_name;

    #[mononoke::test]
    fn test_invalid_workspace_names() {
        assert!(!sanity_check_workspace_name("user_testuser_default"));
        assert!(!sanity_check_workspace_name("user_testuser/default"));
        assert!(!sanity_check_workspace_name("user/testuser_default"));
        assert!(!sanity_check_workspace_name("user/testuser@/default"));
        assert!(!sanity_check_workspace_name(
            "user/test user@oculus.com/default"
        ));
        assert!(!sanity_check_workspace_name("user/[[[user@fb.com/default"));
    }

    #[mononoke::test]
    fn test_valid_workspace_names() {
        assert!(sanity_check_workspace_name("user/testuser/default"));
        assert!(sanity_check_workspace_name(
            "user/testuser@oculus.com/default"
        ));
        assert!(sanity_check_workspace_name(
            "user/testuser@oculus.com/othername"
        ));
        assert!(sanity_check_workspace_name(
            "user/testuser@oculus.com/other name with spaces"
        ));
    }

    #[mononoke::test]
    fn test_invalid_acl_names() {
        assert!(!is_valid_acl_name(
            "user/testuser@oculus.com/other name with spaces"
        ));
        assert!(!is_valid_acl_name("user/testuser@oculus.com/default"));
    }

    #[mononoke::test]
    fn test_valid_acl_names() {
        assert!(is_valid_acl_name("user/testuser/default"));
    }

    #[mononoke::test]
    fn test_workspace_name_decorate_for_acl() {
        assert_eq!(
            decorate_workspace_name_to_valid_acl_name("user/testuser/default"),
            "user/testuser/default"
        );
        assert_eq!(
            decorate_workspace_name_to_valid_acl_name("user/testuser@oculus.com/default"),
            "user/testuser_oculus.com/default"
        );
        assert_eq!(
            decorate_workspace_name_to_valid_acl_name(
                "user/testuser@oculus.com/other name with spaces"
            ),
            "user/testuser_oculus.com/other_name_with_spaces"
        );
    }
}
