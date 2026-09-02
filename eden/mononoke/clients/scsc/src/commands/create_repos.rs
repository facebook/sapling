/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::bail;
use scs_client_raw::thrift;

use crate::ScscApp;

#[derive(clap::Parser)]
/// Create git repos via the SCS create_repos API (admin-only)
pub(super) struct CommandArgs {
    /// Hipster group to use for newly created ACL (if not specified, will not create new ACL)
    #[clap(long)]
    hipster_group: Option<String>,
    /// Oncall owning the repo
    #[clap(long)]
    oncall_name: String,
    /// Expected size of the repos, used to provision resources for them
    #[clap(long, value_enum, default_value = "small")]
    size_bucket: SizeBucket,
    /// Short branch name (e.g. "main", not "refs/heads/main") the new repos'
    /// HEAD symref points at; unset means clones have no default branch
    #[clap(long)]
    default_branch: Option<String>,
    /// Names of the repos to create
    repo_names: Vec<String>,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum SizeBucket {
    /// <100MB
    ExtraSmall,
    /// <1GB
    Small,
    /// <10GB
    Medium,
    /// <100GB
    Large,
    /// >100GB
    ExtraLarge,
}

impl From<SizeBucket> for thrift::RepoSizeBucket {
    fn from(size_bucket: SizeBucket) -> Self {
        match size_bucket {
            SizeBucket::ExtraSmall => thrift::RepoSizeBucket::EXTRA_SMALL,
            SizeBucket::Small => thrift::RepoSizeBucket::SMALL,
            SizeBucket::Medium => thrift::RepoSizeBucket::MEDIUM,
            SizeBucket::Large => thrift::RepoSizeBucket::LARGE,
            SizeBucket::ExtraLarge => thrift::RepoSizeBucket::EXTRA_LARGE,
        }
    }
}

fn build_requests(args: &CommandArgs) -> Vec<thrift::RepoCreationRequest> {
    args.repo_names
        .iter()
        .map(|repo_name| thrift::RepoCreationRequest {
            repo_name: repo_name.clone(),
            scm_type: thrift::RepoScmType::GIT,
            oncall_name: args.oncall_name.clone(),
            custom_acl: args
                .hipster_group
                .as_ref()
                .map(|hipster_group| thrift::CustomAclParams {
                    hipster_group: hipster_group.clone(),
                    ..Default::default()
                }),
            size_bucket: args.size_bucket.into(),
            default_branch: args.default_branch.clone(),
            ..Default::default()
        })
        .collect()
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let conn = app.get_connection(None).await?;
    let repos = build_requests(&args);
    let params = thrift::CreateReposParams {
        repos,
        ..Default::default()
    };
    let token = conn.create_repos(&params).await?;

    // Repo creation is potentially asynchronous request. Let's poll it until it's done.
    loop {
        let res = conn.create_repos_poll(&token).await?;
        if let Some(result) = res.result {
            match result.status {
                thrift::CreateReposStatus::SUCCESS => {
                    eprintln!("Repo creation succeeded.");
                    break;
                }
                thrift::CreateReposStatus::FAILED => {
                    let msg = result
                        .message
                        .unwrap_or_else(|| "no details provided".to_string());
                    bail!("Repo creation failed: {msg}");
                }
                thrift::CreateReposStatus::ABORTED => {
                    let msg = result
                        .message
                        .unwrap_or_else(|| "no details provided".to_string());
                    bail!("Repo creation aborted: {msg}");
                }
                thrift::CreateReposStatus::IN_PROGRESS => {
                    // Still in progress, keep polling
                }
                status => {
                    bail!("Repo creation returned unexpected status: {status:?}");
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    fn test_args(
        repo_names: &[&str],
        hipster_group: Option<&str>,
        size_bucket: SizeBucket,
    ) -> CommandArgs {
        CommandArgs {
            hipster_group: hipster_group.map(String::from),
            oncall_name: "my_oncall".to_string(),
            size_bucket,
            default_branch: None,
            repo_names: repo_names.iter().map(|name| name.to_string()).collect(),
        }
    }

    #[mononoke::test]
    fn test_hipster_group_populates_custom_acl() {
        let requests = build_requests(&test_args(&["repo1"], Some("my_group"), SizeBucket::Small));
        assert_eq!(requests.len(), 1);
        let custom_acl = requests[0]
            .custom_acl
            .as_ref()
            .expect("custom_acl should be set when a hipster group is given");
        assert_eq!(custom_acl.hipster_group, "my_group");
        assert!(
            requests[0].default_branch.is_none(),
            "default_branch must stay unset when --default-branch is not given"
        );
    }

    #[mononoke::test]
    fn test_no_hipster_group_means_no_custom_acl() {
        let requests = build_requests(&test_args(&["repo1", "repo2"], None, SizeBucket::Small));
        assert_eq!(
            requests
                .iter()
                .map(|r| r.repo_name.as_str())
                .collect::<Vec<_>>(),
            vec!["repo1", "repo2"],
            "each repo name should map to its own request, in order"
        );
        for request in &requests {
            assert!(
                request.custom_acl.is_none(),
                "custom_acl should stay unset when no hipster group is given"
            );
            assert!(
                request.default_branch.is_none(),
                "default_branch should stay unset when not requested"
            );
            assert_eq!(request.oncall_name, "my_oncall");
            assert_eq!(request.scm_type, thrift::RepoScmType::GIT);
        }
    }

    #[mononoke::test]
    fn test_size_bucket_is_passed_through() {
        let requests = build_requests(&test_args(&["repo1"], None, SizeBucket::Large));
        assert_eq!(
            requests[0].size_bucket,
            thrift::RepoSizeBucket::LARGE,
            "size_bucket should be passed through, not hardcoded to SMALL"
        );
    }

    #[mononoke::test]
    fn test_size_bucket_flag_mapping() {
        // The server maps SMALL and MEDIUM to the same t-shirt size; the CLI still sends the exact bucket.
        for (flag, expected) in [
            (SizeBucket::ExtraSmall, thrift::RepoSizeBucket::EXTRA_SMALL),
            (SizeBucket::Small, thrift::RepoSizeBucket::SMALL),
            (SizeBucket::Medium, thrift::RepoSizeBucket::MEDIUM),
            (SizeBucket::Large, thrift::RepoSizeBucket::LARGE),
            (SizeBucket::ExtraLarge, thrift::RepoSizeBucket::EXTRA_LARGE),
        ] {
            assert_eq!(thrift::RepoSizeBucket::from(flag), expected);
        }
    }

    #[mononoke::test]
    fn test_default_branch_flag_parses_and_reaches_request() {
        use clap::Parser;
        let args = CommandArgs::try_parse_from([
            "create-repos",
            "--oncall-name",
            "oc",
            "--default-branch",
            "main",
            "repo1",
        ])
        .expect("args with --default-branch should parse");
        let requests = build_requests(&args);
        assert_eq!(
            requests[0].default_branch.as_deref(),
            Some("main"),
            "--default-branch should reach the request"
        );
    }
}
