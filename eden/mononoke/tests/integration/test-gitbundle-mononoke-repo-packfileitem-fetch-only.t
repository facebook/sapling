# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests", "unodes", "filenodes", "hgchangesets"]' setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
# Create a few commits with changes
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -q -am "Add file1"

  $ git tag -a -m "new tag" first_tag
  $ mkdir src
  $ echo "fn main() -> Result<()>" > src/lib.rs
  $ git add .
  $ git commit -q -m "Added rust library"

  $ git tag -a -m "tag for first release" release_v1.0
  $ mkdir src/test
  $ echo "fn test() -> Result<()>" > src/test/test.rs
  $ echo "mod test.rs" > src/mod.rs
  $ git add .
  $ git commit -q -m "Added rust tests"
  $ echo "This is new rust library. Use it on your own risk" > README.md
  $ git add .
  $ git commit -q -m "Added README.md"
# Create a simple tag to validate its handled properly along with annotated tags
  $ git tag simple_tag

  $ echo "{ let result: Option<usize> = Some(0); if let Some(result) = result { let output = result; } }" > src/lib.rs
  $ mkdir src/pack
  $ echo "New rust code for packing" > src/pack/lib.rs
  $ mkdir src/pack/test
  $ echo "New testing code for packing" > src/pack/test/main.rs
  $ git add .
  $ git commit -q -m "Added basic packing code and tests"

  $ git checkout -qb dev_branch
  $ mkdir -p src/pack
  $ echo "Encoding logic to be used during packing" > src/pack/encode.rs
  $ git add .
  $ git commit -q -m "Added encoding logic in packing"
  $ git tag -a -m "Tag for commit for latest version of dev branch" dev_version

  $ git checkout -qb test_branch
  $ mkdir -p src/pack/test
  $ echo "Utility method for testing" > src/pack/test/helper.rs
  $ git add .
  $ git commit -q -m "Added helper methods for testing"
  $ git tag -a -m "Tag for commit for latest version of tag branch" tag_version

  $ git checkout -q master
  $ git merge -q dev_branch test_branch

  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Get the repository log
  $ git log --pretty=format:"%h %an %s %D" > $TESTTMP/repo_log

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport --record-head-symref "$GIT_REPO" --generate-bookmarks full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:$TESTTMP/repo-git commit 1 of 7 - Oid:8ce3eae4 => Bid:032cd4dc (glob)
  * GitRepo:$TESTTMP/repo-git commit 2 of 7 - Oid:a612a217 => Bid:148f9770 (glob)
  * GitRepo:$TESTTMP/repo-git commit 3 of 7 - Oid:ca4b2b21 => Bid:3e35338a (glob)
  * GitRepo:$TESTTMP/repo-git commit 4 of 7 - Oid:7cb1854d => Bid:70d4bcfd (glob)
  * GitRepo:$TESTTMP/repo-git commit 5 of 7 - Oid:ed15d7e7 => Bid:89b959e7 (glob)
  * GitRepo:$TESTTMP/repo-git commit 6 of 7 - Oid:3d0b9959 => Bid:a2cfb9ad (glob)
  * GitRepo:$TESTTMP/repo-git commit 7 of 7 - Oid:e460783b => Bid:73a90516 (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/remotes/origin/HEAD": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/remotes/origin/dev_branch": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/remotes/origin/master": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/remotes/origin/test_branch": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/tags/dev_version": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/tags/release_v1.0": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/tags/simple_tag": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/tags/tag_version": Some(ChangesetId(Blake2(*))) (glob)
  * Initializing repo: repo (glob)
  * Initialized repo: repo (glob)
  * All repos initialized. It took: 0 seconds (glob)
  * Bookmark: "heads/master": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "heads/dev_branch": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "heads/master": ChangesetId(Blake2(*)) (already up-to-date) (glob)
  * Bookmark: "heads/test_branch": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "tags/dev_version": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "tags/first_tag": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "tags/release_v1.0": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "tags/simple_tag": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "tags/tag_version": ChangesetId(Blake2(*)) (created) (glob)

# Regenerate the Git repo out of the Mononoke repo using stored packfile items and verify that it works
  $ mononoke_newadmin git-bundle create from-repo -R repo --output-location "$BUNDLE_PATH" --packfile-item-inclusion fetch-only
# Ensure that Git considers this a valid bundle
  $ cd $GIT_REPO
  $ git bundle verify $BUNDLE_PATH
  $TESTTMP/repo_bundle.bundle is okay
  The bundle contains these 9 refs:
  * (glob)
  * (glob)
  * (glob)
  * (glob)
  * (glob)
  * (glob)
  * (glob)
  * (glob)
  * (glob)
  The bundle records a complete history.

# Create a new empty folder for containing the repo
  $ mkdir $TESTTMP/git_packfile_item_repo  
  $ cd "$TESTTMP"
  $ git clone "$BUNDLE_PATH" git_packfile_item_repo
  Cloning into 'git_packfile_item_repo'...
  $ cd git_packfile_item_repo

# Get the repository log and verify if its the same as earlier
  $ git log --pretty=format:"%h %an %s %D" > $TESTTMP/new_repo_log
  $ diff -w $TESTTMP/new_repo_log $TESTTMP/repo_log

# Dump all the known Git objects into a file
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list

# Ensure that there are no differences between the set of objects by diffing both object list files
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list 
