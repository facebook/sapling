// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use serde_json::Value;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::utils::build_git_command;
use crate::utils::build_scsc_command;
use crate::utils::run_command;
use crate::utils::run_git_command;
use crate::utils::run_scsc_command;

const NEW_BRANCH_STR: &str = "[new branch]";

/// Message returned by the server when a commit is successfully uploaded to
/// commit cloud.
const COMMIT_CLOUD_MSG: &str =
    "Commit-cloud upload succeeded. Your commit is now backed up in Mononoke";

/// Whether the push is to an existing branch or a new branch,
/// In Mononoke terms: will it land changes to an existing bookmark or create a
/// new one.
#[derive(Debug, Clone)]
enum PushKind {
    LandStack {
        /// Bottom commit in the stack.
        /// For example, if the stack branching from master is `A-B-C`, then the
        /// base is `A`.
        bottom_commit: String,
        /// Top commit in the stack. In the example above, it is `C`.
        top_commit: String,
    },
    /// A bookmark will be created pointing to the provided commit
    CreateOrMoveBookmark(String),
}

/// Data about the refs and revisions being pushed.
#[derive(Debug, Clone)]
struct PushData {
    /// Ref being pushed to the remote repository
    pub refspec_src: String,
    /// Abbreviated remote repository ref to be updated.
    /// Used to determine which large repo bookmark to push to and
    pub abbrev_dst: String,
    pub push_kind: PushKind,
}

/// Use SCS client to sync and push commit to the large repo.
pub(crate) async fn push_to_large_repo(
    git_repo_name: &str,
    mb_remote: Option<String>,
    mb_git_refspec: Option<String>,
    large_repo_pushrebase_bookmark: &str,
    large_repo_name: &str,
) -> Result<()> {
    let remote = mb_remote.unwrap_or("origin".to_string());
    let git_refspec = mb_git_refspec.unwrap_or(format!("HEAD:{large_repo_pushrebase_bookmark}"));

    let push_data = get_push_data(&remote, git_refspec.clone(), large_repo_pushrebase_bookmark)?;

    debug!("push_data: {push_data:#?}");

    // STEP 1: Upload commit to Mononoke
    upload_to_mononoke(&remote, &push_data)?;

    let abbrev_dst = &push_data.abbrev_dst;

    // STEP 2: Land stack or create bookmark
    let new_head = land_changes(
        git_repo_name,
        abbrev_dst,
        large_repo_pushrebase_bookmark,
        &push_data.push_kind,
    )?;

    println!("Branch {remote}/{abbrev_dst} was updated to {new_head}");
    println!(
        "Your changes pushed to {large_repo_name} and automatically rebased before being synced back to {git_repo_name}."
    );
    println!("This changed the commit dates and means that your local copy is now out of sync.");
    println!(
        "Update your local copy by running `git pull --rebase` or `git fetch {remote} {abbrev_dst}` and `git reset --hard {remote}/{abbrev_dst}`"
    );

    Ok(())
}

/// Get the source and destination refspecs and the top and base revisions being
/// pushed.
fn get_push_data(
    remote: &str,
    git_refspec: String,
    large_repo_pushrebase_bookmark: &str,
) -> Result<PushData> {
    let dry_push_stdout =
        run_git_command(["push", remote, &git_refspec, "--dry-run", "--porcelain"])?;

    let push_info_line = dry_push_stdout
        .split('\n')
        .find(|line| line.contains(":") && (line.contains("..") || line.contains("[new branch]")))
        .ok_or_else(|| {
            warn!("To debug this issue, run `git push --dry-run --porcelain`");
            warn!("dry_push output: {dry_push_stdout:#?}");
            anyhow!("No line containing refs and revisions")
        })?;

    // Typical output when pushing an existing branch:
    // `    refs/heads/master:refs/heads/master	408f79c..1a71796`
    // Typical output when pushing a new branch
    // `*   refs/heads/new_branch:refs/heads/new_branch [new branch]`
    let is_new_branch = push_info_line.contains(NEW_BRANCH_STR);
    let mut line_elems = push_info_line.split_whitespace();
    if is_new_branch {
        line_elems.next(); // Drop the `*` printed when creating a new branch
    }
    let refspecs = line_elems.next().ok_or(anyhow!("No refspecs in line"))?;
    let src_dst = refspecs.split(':').collect::<Vec<_>>();
    let (refspec_src, refspec_dst) = match src_dst[..] {
        [refspec_src, refspec_dst] => Ok((refspec_src.to_string(), refspec_dst.to_string())),
        _ => Err(anyhow!("Failed to parse refspecs")),
    }?;

    let abbrev_dst =
        run_git_command(["rev-parse", "--abbrev-ref", &refspec_dst]).unwrap_or_else(|_| {
            warn!("Failed to get abbreviated destination ref for {refspec_dst}");
            let stripped_prefix = refspec_dst.strip_prefix("refs/heads/");

            stripped_prefix.unwrap_or_else(|| {
                warn!("Destination ref is not prefixed with refs/heads. Using {refspec_dst} as abbreviated ref");
                &refspec_dst
            }).to_string()
        });

    debug!("abbrev_dst: {abbrev_dst}");

    let push_kind = if abbrev_dst == large_repo_pushrebase_bookmark {
        let new_commits_raw = run_git_command([
            "rev-list",
            format!("{remote}/{abbrev_dst}..{refspec_src}").as_str(),
        ])?;
        let new_commits = new_commits_raw.split("\n").collect::<Vec<_>>();
        let top_commit = new_commits
            .first()
            .ok_or(anyhow!("No base revision"))?
            .trim()
            .to_string();
        let bottom_commit = new_commits
            .last()
            .ok_or(anyhow!("No top revision"))?
            .trim()
            .to_string();

        ensure!(
            !bottom_commit.is_empty() && !top_commit.is_empty(),
            "Failed to get base and top commits to push to {large_repo_pushrebase_bookmark}. bottom_commit: {bottom_commit}, top_commit: {top_commit}"
        );
        PushKind::LandStack {
            bottom_commit,
            top_commit,
        }
    } else {
        // Creating a new branch with commits that are already in the remote
        let target_commit = run_git_command(["rev-parse", &refspec_src])?;
        PushKind::CreateOrMoveBookmark(target_commit)
    };

    let res = PushData {
        refspec_src,
        abbrev_dst,
        push_kind,
    };

    Ok(res)
}

/// Upload the git commits to the Mononoke repo by pushing it to the commitcloud
/// branch.
/// Returns the list of git commits that were uploaded.
fn upload_to_mononoke(remote: &str, push_data: &PushData) -> Result<()> {
    info!("\nUploading commits to Mononoke");

    let PushData { refspec_src, .. } = push_data;

    // Force push to the commitcloud branch, which uploads the commits to Mononoke
    let refspec = format!("{refspec_src}:refs/commitcloud/upload");

    // The push can fail, so we purposely ignore the result to avoid crashing
    // the process.
    let git_cmd = build_git_command()?;
    let upload_res = run_command(
        git_cmd,
        ["push", remote, &refspec],
        "git",
        true, // quiet
    );

    if let Err(e) = upload_res {
        if !e.to_string().contains(COMMIT_CLOUD_MSG) {
            bail!("Failed to upload commits to Mononoke: {e:#?}");
        }
    }

    Ok(())
}

/// Given the commits that will be landed, get the parent of the bottom commit,
/// as it will be the base commit in the `scsc land-stack` call.
fn get_stack_base(bottom_commit: String, large_repo_name: &str) -> Result<String> {
    let info_stdout = run_scsc_command([
        "info",
        "--repo",
        large_repo_name,
        "-i",
        &bottom_commit,
        "-S",
        "git",
        "--json",
    ])?;

    let info_json: Value =
        serde_json::from_str(&info_stdout).context("Failed to parse scsc info output")?;

    let parent_bonsai = info_json["parents"][0]["git"]
        .as_str()
        .context("The commit has no parents!")?;

    Ok(parent_bonsai.to_string())
}

/// Use SCS client to land all the synced commits on the large repo
///
/// If the target bookmark is the common pushrebase bookmark, use `scsc land-stack`
/// Otherwise, use `scsc move-bookmark` or `scsc create-bookmark` depending if
/// the bookmark already exists.
fn land_changes(
    git_repo_name: &str,
    dst_bookmark: &str,
    large_repo_pushrebase_bookmark: &str,
    push_kind: &PushKind,
) -> Result<String> {
    let target_bookmark = format!("heads/{dst_bookmark}");

    let (bottom_commit, top_commit) = match push_kind {
        PushKind::LandStack {
            bottom_commit,
            top_commit,
        } => (bottom_commit.to_string(), top_commit),
        PushKind::CreateOrMoveBookmark(target_commit) => {
            if dst_bookmark == large_repo_pushrebase_bookmark {
                return Err(anyhow!(
                    "Cannot move bookmark {dst_bookmark} in large repo without pushrebase"
                ));
            }
            // All commits are already in the server, so we just need to move or
            // set the bookmark
            create_or_move_bookmark(git_repo_name, &target_bookmark, target_commit)?;
            return Ok(target_commit.to_string());
        }
    };

    info!(
        "Pushing stack from {bottom_commit} to {top_commit} to bookmark {target_bookmark} using pushrebase"
    );
    let base_commit = get_stack_base(bottom_commit.to_string(), git_repo_name)?;

    let land_stack_out = run_scsc_command([
        "land-stack",
        "--repo",
        git_repo_name,
        "--name",
        &target_bookmark,
        "-i",
        top_commit,
        "-i",
        &base_commit,
        "-S",
        "git",
        "--json",
    ])?;

    let land_stack_json: Value =
        serde_json::from_str(&land_stack_out).context("Failed to parse scsc land_stack output")?;

    if let Some(new_head) = land_stack_json["head"]["bonsai"].as_str() {
        info!("New head of bookmark {target_bookmark} in {git_repo_name} is {new_head}");
        Ok(new_head.to_string())
    } else {
        Err(anyhow!("No commits landed by scsc land-stack"))
    }
}

/// Use SCS to either move or create a bookmark.
/// If the bookmark already exists, this is used instead of land-stack to move
/// the bookmark without pushrebase, so this shouldn't be called with common
/// pushrebase bookmarks (e.g. master).
///
/// The repo is push-redirected, so push-redirection will handle the cross-repo
/// syncing and waiting for backsync transparently.
fn create_or_move_bookmark(
    git_repo_name: &str,
    target_bookmark: &str,
    target_commit: &str,
) -> Result<()> {
    let cmd = build_scsc_command();
    let existing_bookmarks = run_command(
        cmd,
        [
            "info",
            "--repo",
            git_repo_name,
            "-S",
            "git",
            "-B",
            target_bookmark,
            "--json",
        ],
        "scsc",
        true, // quiet
    );

    let mut cmd = build_scsc_command();
    cmd.env("SCSC_WRITES_ENABLED", "1");

    let bookmark_cmd = if existing_bookmarks.is_ok() {
        debug!("Moving bookmark {target_bookmark} to {target_commit}");
        "move-bookmark"
    } else {
        debug!("Creating bookmark {target_bookmark} pointing to {target_commit}");
        "create-bookmark"
    };

    let _res = run_command(
        cmd,
        [
            bookmark_cmd,
            "--repo",
            git_repo_name,
            "--name",
            target_bookmark,
            "-i",
            target_commit,
            "--json",
        ],
        "scsc",
        false, // quiet
    )?;

    Ok(())
}
