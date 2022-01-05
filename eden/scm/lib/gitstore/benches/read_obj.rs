/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use gitstore::GitStore;
use minibench::bench;
use minibench::elapsed;
use types::HgId;

fn main() {
    let git_dir = match std::env::var("GIT_DIR") {
        Ok(dir) => PathBuf::from(dir),
        _ => {
            eprintln!("This benchmark requires a git repo set by GIT_DIR.");
            eprintln!("Set GIT_DIR and re-run the benchmark.");
            return;
        }
    };

    // Get some commit hashes from the repo.
    eprint!("obtaining commit hashes\r");
    let commits_str: String = git(&["log", "--format=%H", "--max-count=10000"]);
    let commit_hgids: Vec<HgId> = commits_str
        .lines()
        .map(|l| HgId::from_hex(l.as_bytes()).unwrap())
        .collect();
    eprintln!("got {:5} commit hashes", commit_hgids.len());

    bench("reading commit objects via GitStore", || {
        let store = GitStore::open(&git_dir).unwrap();
        elapsed(|| {
            for &id in &commit_hgids {
                let _ = store.read_obj(id, git2::ObjectType::Commit).unwrap();
            }
        })
    });

    bench("reading commit objects via git CLI", || {
        elapsed(|| {
            git_cat_file(&commits_str);
        })
    });
}

fn git(args: &[&str]) -> String {
    let out = Command::new("git").args(args).output().unwrap();
    String::from_utf8(out.stdout).unwrap()
}

fn git_cat_file(input: &str) {
    let mut proc = Command::new("git")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .args(["cat-file", "--batch"])
        .spawn()
        .unwrap();
    proc.stdin
        .as_ref()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    proc.wait().unwrap();
}
