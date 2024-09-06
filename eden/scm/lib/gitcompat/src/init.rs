/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::path::MAIN_SEPARATOR_STR as SEP;

use anyhow::Result;
use filetime::set_file_mtime;
use filetime::FileTime;
use fs_err as fs;
use identity::Identity;
use tracing::debug;
use types::HgId;

use crate::rungit::GlobalGit;
use crate::utils::follow_dotgit_path;

/// Initialize Sapling's dotdir inside `.git/`. Write requirements. Update config files from
/// translated Git config.
/// Skip if the directory already exists, or if `ident` is not using `.git/sl` dot dir.
///
/// `dot_dir` is expected to be something like `<prefix>/.git/sl`.
pub fn maybe_init_inside_dotgit(root_path: &Path, ident: Identity) -> Result<()> {
    let dot_dir = ident.dot_dir();
    if dot_dir != ".git/sl" {
        return Ok(());
    }

    let dot_git_path = follow_dotgit_path(root_path.join(".git"));
    let dot_dir = dot_git_path.join("sl");
    let store_dir = dot_dir.join("store");
    if !store_dir.is_dir() {
        fs::create_dir_all(&store_dir)?;

        fs::write(dot_dir.join("requires"), "store\ndotgit\n")?;
        fs::write(
            store_dir.join("requires"),
            "narrowheads\nvisibleheads\ngit\ngit-store\ndotgit\n",
        )?;
        fs::write(store_dir.join("gitdir"), format!("..{SEP}.."))?;

        // Write an empty eden dirstate so it can be loaded.
        treestate::overlay_dirstate::write_overlay_dirstate(
            &dot_dir.join("dirstate"),
            std::iter::once(("p1".to_owned(), HgId::null_id().to_hex())).collect(),
            Default::default(),
        )?;
    }

    // Sync git config to "config-git-user", "config-git-repo".
    // Skip if file mtime is up to date (since shelling out to `git config` might take time).
    let user_config_path = translated_git_user_config_path(&dot_dir, ident);
    let repo_config_path = translated_git_repo_config_path(&dot_dir, ident);
    let git_repo_mtime = git_repo_config_mtime(&dot_git_path);
    let git_user_mtime = git_user_config_mtime();

    if git_repo_mtime != try_mtime(&repo_config_path)
        || git_user_mtime != try_mtime(&user_config_path)
    {
        debug!("translating git configs");
        // NOTE: At this point no sapling config is loaded. For simplicity, this does not respect `ui.git`.
        let git = GlobalGit::from_config(&BTreeMap::<String, String>::new());
        let (user_config, repo_config) = git.translate_git_config()?;
        fs::write(&user_config_path, user_config)?;
        fs::write(&repo_config_path, repo_config)?;
        set_file_mtime(&user_config_path, git_user_mtime)?;
        set_file_mtime(&repo_config_path, git_repo_mtime)?;
    } else {
        debug!("skipped translating git configs");
    }

    Ok(())
}

fn git_repo_config_mtime(dot_git_path: &Path) -> FileTime {
    try_mtime(&dot_git_path.join("config"))
}

// NOTE: This currently does not consider corner cases, including:
// - XDG_CONFIG_HOME config file changes
// - system config file changes
// - "(conditional) include" config files (https://git-scm.com/docs/git-config#_includes)
fn git_user_config_mtime() -> FileTime {
    let home = match env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        Err(_) => return FileTime::zero(),
        Ok(v) => v,
    };
    try_mtime(Path::new(&format!("{home}{SEP}.gitconfig")))
}

fn try_mtime(path: &Path) -> FileTime {
    match fs::metadata(path) {
        Err(_) => FileTime::zero(),
        Ok(m) => FileTime::from_last_modification_time(&m),
    }
}

/// Obtain path to the sapling config translated from git's user config.
pub fn translated_git_user_config_path(dot_sl_path: &Path, ident: Identity) -> PathBuf {
    dot_sl_path.join(format!("{}-git-user", ident.config_repo_file()))
}

/// Obtain path to the sapling config translated from git's repo config.
pub fn translated_git_repo_config_path(dot_sl_path: &Path, ident: Identity) -> PathBuf {
    dot_sl_path.join(format!("{}-git-repo", ident.config_repo_file()))
}
