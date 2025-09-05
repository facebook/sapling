/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::anyhow;
use configmodel::config::ConfigExt;
use cxx::SharedPtr;
use cxx::UniquePtr;
use manifest::FileMetadata;
use manifest::FsNodeMetadata;
use manifest::Manifest;
use metrics::Counter;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use pathmatcher::DirectoryMatch;
use pathmatcher::TreeMatcher;
use repo::repo::Repo;
use sparse::Matcher;
use sparse::Root;
use types::FetchContext;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;

use crate::ffi::MatcherPromise;
use crate::ffi::MatcherWrapper;
use crate::ffi::set_matcher_error;
use crate::ffi::set_matcher_promise_error;
use crate::ffi::set_matcher_promise_result;
use crate::ffi::set_matcher_result;

struct CachedRepo {
    repo: Repo,
    expiration: Option<Instant>,
}

static REPO_HASHMAP: Lazy<Mutex<HashMap<PathBuf, CachedRepo>>> = Lazy::new(|| {
    let map = Mutex::new(HashMap::new());

    // Start the cleanup thread that checks for evictions every ~15 seconds
    thread::spawn(cleanup_expired_repos);

    map
});

static LOOKUPS: Counter = Counter::new_counter("edenffi.ffs.lookups");
static LOOKUP_FAILURES: Counter = Counter::new_counter("edenffi.ffs.lookup_failures");
static INVALID_REPO: Counter = Counter::new_counter("edenffi.ffs.invalid_repo");
static REPO_CACHE_MISSES: Counter = Counter::new_counter("edenffi.ffs.repo_cache_misses");
static REPO_CACHE_HITS: Counter = Counter::new_counter("edenffi.ffs.repo_cache_hits");
static REPO_CACHE_CLEANUPS: Counter = Counter::new_counter("edenffi.ffs.repo_cache_cleanups");

const DEFAULT_CACHE_EXPIRY_DURATION: Duration = Duration::from_secs(300); // 5 minutes

// Background thread to clean up expired repos
fn cleanup_expired_repos() {
    loop {
        // Check for expired cache entries every 15 seconds
        thread::sleep(Duration::from_secs(15));

        let mut repo_map = REPO_HASHMAP.lock();
        let now = Instant::now();
        let initial_count = repo_map.len();

        // Only keep repos that aren't expired
        repo_map.retain(|_path, cached_repo| cached_repo.expiration.is_none_or(|exp| now < exp));

        let final_count = repo_map.len();
        drop(repo_map);

        let cleaned_count = initial_count.saturating_sub(final_count);
        if cleaned_count > 0 {
            tracing::info!(
                "Cleaned up {} expired repo entries from cache",
                cleaned_count
            );
            REPO_CACHE_CLEANUPS.add(cleaned_count);
        }
    }
}

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
    matcher: Box<dyn pathmatcher::Matcher>,
}

impl MercurialMatcher {
    // Returns true if the given path and all of its children are unfiltered
    fn matches_directory(
        self: &MercurialMatcher,
        path: &str,
    ) -> Result<ffi::FilterDirectoryMatch, anyhow::Error> {
        let repo_path = RepoPath::from_str(path)?;
        // This is tricky -- a filter file defines which files should be *excluded* from the repo.
        // The filtered files are put in the [exclude] section of the file. So, if something is
        // recursively unfiltered, then it means that there are no exclude patterns that match it.
        let res = self.matcher.matches_directory(repo_path)?;
        Ok(res.into())
    }

    fn matches_file(self: &MercurialMatcher, path: &str) -> Result<bool, anyhow::Error> {
        let repo_path = RepoPath::from_str(path)?;
        self.matcher.matches_file(repo_path)
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
        include!("eden/scm/lib/edenfs_ffi/include/ffi.h");

        #[namespace = "facebook::eden"]
        type MatcherPromise;

        #[namespace = "facebook::eden"]
        type MatcherWrapper;

        #[namespace = "facebook::eden"]
        fn set_matcher_promise_result(
            promise: UniquePtr<MatcherPromise>,
            value: Box<MercurialMatcher>,
        );

        #[namespace = "facebook::eden"]
        fn set_matcher_promise_error(promise: UniquePtr<MatcherPromise>, error: String);

        #[namespace = "facebook::eden"]
        fn set_matcher_result(wrapper: SharedPtr<MatcherWrapper>, value: Box<MercurialMatcher>);

        #[namespace = "facebook::eden"]
        fn set_matcher_error(wrapper: SharedPtr<MatcherWrapper>, error: String);
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
        fn matches_directory(self: &MercurialMatcher, path: &str) -> Result<FilterDirectoryMatch>;

        // Returns true if the given path is unfiltered.
        fn matches_file(self: &MercurialMatcher, path: &str) -> Result<bool>;

        fn create_tree_matcher(
            globs: Vec<String>,
            case_sensitive: bool,
            matcher_wrapper: SharedPtr<MatcherWrapper>,
        ) -> Result<()>;
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

fn create_tree_matcher(
    globs: Vec<String>,
    case_sensitive: bool,
    matcher_wrapper: SharedPtr<MatcherWrapper>,
) -> Result<(), anyhow::Error> {
    let matcher = TreeMatcher::from_rules(globs.iter(), case_sensitive)?;
    let mercurial_matcher = Ok(Box::new(MercurialMatcher {
        matcher: Box::new(matcher),
    }));
    match mercurial_matcher {
        Ok(m) => set_matcher_result(matcher_wrapper, m),
        Err(e) => set_matcher_error(matcher_wrapper, e),
    };
    Ok(())
}

// As mentioned below, we return the MercurialMatcher via a promise to circumvent some async
// limitations in CXX. This function wraps the bulk of the Sparse logic and provides a single
// place for returning result/error info via the MatcherPromise.
fn profile_contents_from_repo(
    id: FilterId,
    abs_repo_path: PathBuf,
    promise: UniquePtr<MatcherPromise>,
) {
    match _profile_contents_from_repo(id, abs_repo_path) {
        Ok(res) => {
            set_matcher_promise_result(promise, res);
        }
        Err(e) => {
            set_matcher_promise_error(promise, format!("Failed to get filter: {:?}", e));
        }
    };
}

// Fetches the content of a filter file and turns it into a MercurialMatcher
fn _profile_contents_from_repo(
    id: FilterId,
    abs_repo_path: PathBuf,
) -> Result<Box<MercurialMatcher>, anyhow::Error> {
    let mut repo_map = REPO_HASHMAP.lock();
    if !repo_map.contains_key(&abs_repo_path) {
        // Load the repo and store it for later use
        REPO_CACHE_MISSES.increment();
        let repo = Repo::load(&abs_repo_path, &[]).with_context(|| {
            anyhow!("failed to load Repo object for {}", abs_repo_path.display())
        })?;
        let ttl = repo
            .config()
            .must_get("edenfs", "ffs-repo-cache-ttl")
            .unwrap_or(DEFAULT_CACHE_EXPIRY_DURATION);
        let expiration = if ttl == Duration::ZERO {
            None
        } else {
            Some(Instant::now() + ttl)
        };
        repo_map.insert(abs_repo_path.clone(), CachedRepo { repo, expiration });
    } else {
        REPO_CACHE_HITS.increment();
    }

    let cached_repo = repo_map.get_mut(&abs_repo_path).context("loading repo")?;
    let repo = &mut cached_repo.repo;

    // Create the tree manifest for the root tree of the repo
    let tree_manifest = match repo.tree_resolver()?.get(&id.hg_id) {
        Ok(manifest_id) => manifest_id,
        Err(e) => {
            // It's possible that the commit exists but was only recently
            // created. Invalidate the in-memory commit graph and force a read
            // from disk. Note: This can be slow, so only do it on error.
            repo.invalidate_all()?;
            repo.tree_resolver()?.get(&id.hg_id).with_context(|| {
                anyhow!(
                    "Failed to get root tree id for commit {:?}: {:?}",
                    &id.hg_id,
                    e
                )
            })?
        }
    };

    let repo_store = repo
        .file_store()
        .context("failed to get FileStore from Repo object")?;

    // Get the metadata of the filter file and verify it's a valid file.
    let p = id.repo_path.clone();

    let matcher = async_runtime::block_in_place(|| -> anyhow::Result<_> {
        let metadata = tree_manifest.get(&p)?;
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

        let data = repo_store
            .get_content(FetchContext::default(), &id.repo_path, file_id)?
            .into_bytes();

        // We no longer need to hold the lock on the repo_map
        drop(repo_map);

        let mut root = Root::single_profile(data, id.repo_path.to_string())?;
        root.set_version_override(Some("2".to_owned()));
        let matcher = root.matcher(|_| Ok(Some(vec![])))?;
        Ok(matcher)
    });

    // If the result is an error, then the filter file doesn't exist or is
    // invalid. Return an always matcher instead of erroring out.
    let sparse_matcher = matcher.unwrap_or_else(|e| {
        tracing::warn!("Failed to get sparse matcher for active filter: {:?}", e);
        LOOKUP_FAILURES.increment();
        Matcher::new(
            vec![TreeMatcher::always()],
            vec![vec!["always_matcher".to_string()]],
        )
    });

    Ok(Box::new(MercurialMatcher {
        matcher: Box::new(sparse_matcher),
    }))
}

// CXX doesn't allow async functions to be exposed to C++. This function wraps the bulk of the
// Sparse Profile creation logic.
pub fn profile_from_filter_id(
    id: &str,
    checkout_path: &str,
    promise: UniquePtr<MatcherPromise>,
) -> Result<(), anyhow::Error> {
    LOOKUPS.increment();

    // Parse the FilterID
    let filter_id = FilterId::from_str(id)?;

    // We need to verify the checkout exists. The passed in checkout_path
    // should correspond to a valid hg/sl repo that Mercurial is aware of.
    let abs_repo_path = PathBuf::from(checkout_path);
    if identity::sniff_dir(&abs_repo_path).is_err() {
        INVALID_REPO.increment();
        return Err(anyhow!(
            "{} is not a valid hg repo",
            abs_repo_path.display()
        ));
    }

    // If we've already loaded a filter from this repo before, we can skip Repo
    // object creation. Otherwise, we need to pay the 1 time cost of creating
    // the Repo object.
    profile_contents_from_repo(filter_id, abs_repo_path, promise);

    Ok(())
}
