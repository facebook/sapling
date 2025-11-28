// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

#![cfg(test)]

use mononoke_macros::mononoke;

use super::*;

fn run_git_repo_name_test(
    remote_name: &str,
    remote_output_lines: &[&str],
    expected: Option<&str>,
) -> Result<()> {
    let remote_out = remote_output_lines.join("\n");

    println!("Testing remote_output:\n{}", remote_out);
    let res = get_git_repo_name_impl(remote_name, remote_out.to_string())?;

    if let Some(expected) = expected {
        assert_eq!(res, Some(expected.to_string()));
    }

    Ok(())
}

#[mononoke::fbinit_test]
fn test_git_repo_name_parsing_simple_repo(_fb: FacebookInit) -> Result<()> {
    let origin = "origin";

    let repo = "project";

    // rw
    let rw_lines = [
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project (fetch)",
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project (push)",
    ];
    run_git_repo_name_test(origin, &rw_lines, Some(repo))?;

    run_git_repo_name_test(
        origin,
        &[
            "origin\thttps://git.internal.tfbnw.net/repos/git/ro/project (fetch)",
            "origin\thttps://git.internal.tfbnw.net/repos/git/ro/project (push)",
        ],
        Some(repo),
    )?;

    let rw_with_git_lines = [
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project.git (fetch)",
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project.git (push)",
    ];
    run_git_repo_name_test(origin, &rw_with_git_lines, Some(repo))?;

    Ok(())
}

#[mononoke::fbinit_test]
fn test_git_repo_name_parsing_repo_with_slash(_fb: FacebookInit) -> Result<()> {
    let origin = "origin";

    let repo_with_slash = "project/server";

    // rw
    let rw_lines = [
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project/server (fetch)",
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project/server (push)",
    ];
    run_git_repo_name_test(origin, &rw_lines, Some(repo_with_slash))?;

    run_git_repo_name_test(
        origin,
        &[
            "origin\thttps://git.internal.tfbnw.net/repos/git/ro/project/server (fetch)",
            "origin\thttps://git.internal.tfbnw.net/repos/git/ro/project/server (push)",
        ],
        Some(repo_with_slash),
    )?;

    // with spaces instead of tabs
    run_git_repo_name_test(
        origin,
        &[
            "origin  https://git.internal.tfbnw.net/repos/git/ro/project/server (fetch)",
            "origin  https://git.internal.tfbnw.net/repos/git/ro/project/server (push)",
        ],
        Some(repo_with_slash),
    )?;

    run_git_repo_name_test(
        origin,
        &[
            "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project/server (push)",
            "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project/server (fetch)",
        ],
        Some(repo_with_slash),
    )?;

    let rw_with_git_lines = [
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project/server.git (fetch)",
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project/server.git (push)",
    ];
    run_git_repo_name_test(origin, &rw_with_git_lines, Some(repo_with_slash))?;

    Ok(())
}

#[mononoke::fbinit_test]
fn test_git_repo_name_parsing_repo_on_laptop(_fb: FacebookInit) -> Result<()> {
    let origin = "origin";

    let repo_with_slash = "project/server";

    // rw
    let rw_lines = [
        "origin\thttp://git.edge.x2p.facebook.net/repos/git/rw/project/server (fetch)",
        "origin\thttp://git.edge.x2p.facebook.net/repos/git/rw/project/server (push)",
    ];
    run_git_repo_name_test(origin, &rw_lines, Some(repo_with_slash))?;

    run_git_repo_name_test(
        origin,
        &[
            "origin  http://git.edge.x2p.facebook.net/repos/git/ro/project/server (fetch)",
            "origin  http://git.edge.x2p.facebook.net/repos/git/ro/project/server (push)",
        ],
        Some(repo_with_slash),
    )?;

    // with spaces instead of tabs
    run_git_repo_name_test(
        origin,
        &[
            "origin  http://git.edge.x2p.facebook.net/repos/git/ro/project/server (fetch)",
            "origin  http://git.edge.x2p.facebook.net/repos/git/ro/project/server (push)",
        ],
        Some(repo_with_slash),
    )?;

    run_git_repo_name_test(
        origin,
        &[
            "origin\thttp://git.edge.x2p.facebook.net/repos/git/rw/project/server (push)",
            "origin\thttp://git.edge.x2p.facebook.net/repos/git/rw/project/server (fetch)",
        ],
        Some(repo_with_slash),
    )?;

    let rw_with_git_lines = [
        "origin\thttp://git.edge.x2p.facebook.net/repos/git/rw/project/server.git (fetch)",
        "origin\thttp://git.edge.x2p.facebook.net/repos/git/rw/project/server.git (push)",
    ];
    run_git_repo_name_test(origin, &rw_with_git_lines, Some(repo_with_slash))?;

    Ok(())
}

#[mononoke::fbinit_test]
fn test_git_repo_name_parsing_repo_with_hyphen(_fb: FacebookInit) -> Result<()> {
    let origin = "origin";

    let repo_with_hyphen = "project-server";

    // rw
    let rw_lines = [
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project-server (fetch)",
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project-server (push)",
    ];
    run_git_repo_name_test(origin, &rw_lines, Some(repo_with_hyphen))?;

    run_git_repo_name_test(
        origin,
        &[
            "origin\thttps://git.internal.tfbnw.net/repos/git/ro/project-server (fetch)",
            "origin\thttps://git.internal.tfbnw.net/repos/git/ro/project-server (push)\n",
        ],
        Some(repo_with_hyphen),
    )?;

    let rw_with_git_lines = [
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project-server.git (fetch)",
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project-server.git (push)",
    ];
    run_git_repo_name_test(origin, &rw_with_git_lines, Some(repo_with_hyphen))?;

    Ok(())
}

#[mononoke::fbinit_test]
fn test_git_repo_name_parsing_multiple_remotes(_fb: FacebookInit) -> Result<()> {
    let origin = "origin";

    let repo_with_slash = "project/server";

    // rw
    let rw_lines = [
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project/server (fetch)",
        "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project/server (push)",
        "backup\thttps://git.internal.tfbnw.net/repos/git/rw/project/server/backup (fetch)",
        "backup\thttps://git.internal.tfbnw.net/repos/git/rw/project/server/backup (push)",
    ];
    run_git_repo_name_test(origin, &rw_lines, Some(repo_with_slash))?;

    Ok(())
}

// --------------- Expected to fail ---------------

#[mononoke::fbinit_test]
fn test_git_repo_name_parsing_no_remotes(_fb: FacebookInit) -> Result<()> {
    let origin = "origin";

    // Empty git remote -v output: no name and won't crash
    assert!(
        run_git_repo_name_test(origin, &[], None)
            .is_err_and(|e| { e.to_string().contains("Remote origin remote not found") })
    );

    Ok(())
}

#[mononoke::fbinit_test]
fn test_git_repo_name_parsing_fails_if_remote_doesnt_exist(_fb: FacebookInit) -> Result<()> {
    assert!(
        run_git_repo_name_test(
            "other_remote",
            &[
                "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project/server (fetch)",
                "origin\thttps://git.internal.tfbnw.net/repos/git/rw/project/server (push)",
            ],
            None
        )
        .is_err_and(|e| e
            .to_string()
            .contains("Remote other_remote remote not found"))
    );

    Ok(())
}
