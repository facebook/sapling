/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
use async_runtime::spawn;
use async_runtime::spawn_blocking;
use cxx::UniquePtr;
use futures::StreamExt;
use manifest::FileMetadata;
use manifest::FsNodeMetadata;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use once_cell::sync::Lazy;
use pathmatcher::DirectoryMatch;
use repo::repo::Repo;
use sparse::Matcher;
use sparse::Root;
use tokio::sync::Mutex;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;

use crate::ffi::set_matcher_promise_error;
use crate::ffi::set_matcher_promise_result;
use crate::ffi::MatcherPromise;

static REPO_HASHMAP: Lazy<Mutex<HashMap<PathBuf, Repo>>> = Lazy::new(|| Mutex::new(HashMap::new()));

// A helper class to parse/validate FilterIDs that are passed to Mercurial
struct FilterId {
    pub repo_path: RepoPathBuf,
    pub hg_id: HgId,
}

impl fmt::Display for FilterId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", &self.repo_path, &self.hg_id)
    }
}

impl FromStr for FilterId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let id_components = s.split(':').collect::<Vec<_>>();
        if id_components.len() != 2 {
            return Err(anyhow!(
                "Invalid filter id, must be in the form {{filter_path}}:{{hgid}}. Found: {}",
                s
            ));
        }
        let repo_path =
            RepoPathBuf::from_string(id_components[0].to_string()).with_context(|| {
                anyhow!(
                    "Invalid repo path found in FilterId: {:?}",
                    id_components[0]
                )
            })?;
        let hg_id = HgId::from_str(id_components[1])
            .with_context(|| anyhow!("Invalid HgID found in FilterId: {:?}", id_components[1]))?;
        Ok(FilterId { repo_path, hg_id })
    }
}

// CXX only allows exposing structures that are defined in the bridge crate.
// Therefore, MercurialMatcher simply serves as a wrapper around the actual Matcher object that's
// passed to C++ and back to Rust
pub struct MercurialMatcher {
    matcher: Matcher,
}

impl MercurialMatcher {
    // Returns true if the given path and all of its children are unfiltered
    fn is_recursively_unfiltered(
        self: &MercurialMatcher,
        path: &str,
    ) -> Result<ffi::FilterDirectoryMatch, anyhow::Error> {
        let repo_path = RepoPath::from_str(path)?;
        // This is tricky -- a filter file defines which files should be *excluded* from the repo.
        // The filtered files are put in the [exclude] section of the file. So, if something is
        // recursively unfiltered, then it means that there are no exclude patterns that match it.
        let res = pathmatcher::Matcher::matches_directory(&self.matcher, repo_path)?;
        Ok(res.into())
    }
}

// It's safe to move MatcherPromises between threads
unsafe impl Send for MatcherPromise {}

// NOTE: While MercurialPromises are safe to move between threads, they cannot be shared between threads.
// Ex: calling setPromise from multiple threads is undefined. Therefore we should avoid marking
// MercurialMatcher as Sync. More info here: https://doc.rust-lang.org/stable/std/marker/trait.Sync.html

#[cxx::bridge]
mod ffi {

    pub enum FilterDirectoryMatch {
        RecursivelyFiltered,
        RecursivelyUnfiltered,
        Unfiltered,
    }

    unsafe extern "C++" {
        include!("eden/scm/lib/edenfs_ffi/src/ffi.h");

        #[namespace = "facebook::eden"]
        type MatcherPromise;

        #[namespace = "facebook::eden"]
        fn set_matcher_promise_result(
            promise: UniquePtr<MatcherPromise>,
            value: Box<MercurialMatcher>,
        );

        #[namespace = "facebook::eden"]
        fn set_matcher_promise_error(promise: UniquePtr<MatcherPromise>, error: String);
    }

    #[namespace = "facebook::eden"]
    extern "Rust" {
        type MercurialMatcher;

        // Takes a filter_id that corresponds to a filter file that's checked
        // into the repo.
        //
        // Note: The corresponding call in C++ will throw if the Rust function
        // returns an error result.
        fn profile_from_filter_id(
            id: &str,
            checkout_path: &str,
            promise: UniquePtr<MatcherPromise>,
        ) -> Result<()>;

        // Returns true if the given path and all of its children are unfiltered.
        fn is_recursively_unfiltered(
            self: &MercurialMatcher,
            path: &str,
        ) -> Result<FilterDirectoryMatch>;
    }
}

impl From<DirectoryMatch> for ffi::FilterDirectoryMatch {
    fn from(dm: DirectoryMatch) -> Self {
        match dm {
            DirectoryMatch::Everything => Self::RecursivelyUnfiltered,
            DirectoryMatch::Nothing => Self::RecursivelyFiltered,
            DirectoryMatch::ShouldTraverse => Self::Unfiltered,
        }
    }
}

// As mentioned below, we return the MercurialMatcher via a promise to circumvent some async
// limitations in CXX. This function wraps the bulk of the Sparse logic and provides a single
// place for returning result/error info via the MatcherPromise.
async fn profile_contents_from_repo(
    id: FilterId,
    abs_repo_path: PathBuf,
    promise: UniquePtr<MatcherPromise>,
) {
    match _profile_contents_from_repo(id, abs_repo_path).await {
        Ok(res) => {
            set_matcher_promise_result(promise, res);
        }
        Err(e) => {
            set_matcher_promise_error(promise, format!("Failed to get filter: {}", e));
        }
    }
}

// Fetches the content of a filter file and turns it into a MercurialMatcher
async fn _profile_contents_from_repo(
    id: FilterId,
    abs_repo_path: PathBuf,
) -> Result<Box<MercurialMatcher>, anyhow::Error> {
    let mut repo_hash = REPO_HASHMAP.lock().await;
    if !repo_hash.contains_key(&abs_repo_path) {
        // Load the repo and store it for later use
        let repo = Repo::load(&abs_repo_path, &[], &[]).with_context(|| {
            anyhow!("failed to load Repo object for {}", abs_repo_path.display())
        })?;
        repo_hash.insert(abs_repo_path.clone(), repo);
    }
    let repo = repo_hash
        .get_mut(&abs_repo_path)
        .expect("repo to be loaded");
    let tree_store = repo
        .tree_store()
        .context("failed to get TreeStore from Repo object")?;
    let repo_store = repo
        .file_store()
        .context("failed to get FileStore from Repo object")?;

    // Create the tree manifest for the root tree of the repo
    let manifest_id = repo
        .get_root_tree_id(id.hg_id)
        .await
        .with_context(|| anyhow!("Failed to get root tree id for commit {:?}", &id.hg_id))?;
    let tree_manifest = TreeManifest::durable(tree_store, manifest_id);

    // Get the metadata of the filter file and verify it's a valid file.
    let p = id.repo_path.clone();

    let metadata = spawn_blocking(move || tree_manifest.get(&p)).await??;
    let file_id = match metadata {
        None => {
            return Err(anyhow!("{:?} is not a valid filter file", id.repo_path));
        }
        Some(fs_node) => match fs_node {
            FsNodeMetadata::File(FileMetadata { hgid, .. }) => hgid,
            FsNodeMetadata::Directory(_) => {
                return Err(anyhow!(
                    "{:?} is a directory, not a valid filter file",
                    id.repo_path
                ));
            }
        },
    };

    // TODO(cuev): Is there a better way to do this?
    let mut stream = repo_store
        .get_content_stream(vec![Key::new(id.repo_path.clone(), file_id)])
        .await;
    match stream.next().await {
        Some(Ok((bytes, _key))) => {
            let bytes = bytes.into_vec();
            let root = Root::from_bytes(bytes, id.repo_path.to_string()).unwrap();
            let matcher = root.matcher(|_| async move { Ok(Some(vec![])) }).await?;
            Ok(Box::new(MercurialMatcher { matcher }))
        }
        Some(Err(err)) => Err(err),
        None => Err(anyhow!("no contents for filter file {}", &id.repo_path)),
    }
}

// CXX doesn't allow async functions to be exposed to C++. This function wraps the bulk of the
// Sparse Profile creation logic. We spawn a task to complete the async work, and then return the
// value to C++ via a promise.
pub fn profile_from_filter_id(
    id: &str,
    checkout_path: &str,
    promise: UniquePtr<MatcherPromise>,
) -> Result<(), anyhow::Error> {
    // Parse the FilterID
    let filter_id = FilterId::from_str(id)?;

    // TODO(cuev): Is this even worth doing?
    // We need to verify the checkout exists. The passed in checkout_path
    // should correspond to a valid hg/sl repo that Mercurial is aware of.
    let abs_repo_path = PathBuf::from(checkout_path);
    if identity::sniff_dir(&abs_repo_path).is_err() {
        return Err(anyhow!(
            "{} is not a valid hg repo",
            abs_repo_path.display()
        ));
    }

    // If we've already loaded a filter from this repo before, we can skip Repo
    // object creation. Otherwise, we need to pay the 1 time cost of creating
    // the Repo object.
    spawn(profile_contents_from_repo(
        filter_id,
        abs_repo_path,
        promise,
    ));
    Ok(())
}
