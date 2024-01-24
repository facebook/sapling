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
  $ HG_REPO="${TESTTMP}/repo-hg"
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repsitory
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ git commit --allow-empty -m "root commit"
  [master (root-commit) d53a2ef] root commit
  $ git branch root

  $ echo "this is master" > master
  $ git add master
  $ git commit -qam "Add master"

  $ git checkout -q root
  $ git checkout -qb branch1
  $ echo "this is branch1" > branch1
  $ git add branch1
  $ git commit -qam "Add branch1"

  $ git checkout -q root
  $ git checkout -qb branch2
  $ echo "this is branch2" > branch2
  $ git add branch2
  $ git commit -qam "Add branch2"

  $ git checkout -q master
  $ git merge branch1 branch2
  Trying simple merge with branch1
  Trying simple merge with branch2
  Merge made by the 'octopus' strategy.
   branch1 | 1 +
   branch2 | 1 +
   2 files changed, 2 insertions(+)
   create mode 100644 branch1
   create mode 100644 branch2

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Get the repository log
  $ git log --pretty=format:"%h %an %s %D"
  6283891 mononoke Merge branches 'branch1' and 'branch2' HEAD -> master
  161a8cb mononoke Add master 
  bf946c8 mononoke Add branch1 branch1
  933c6d8 mononoke Add branch2 branch2
  d53a2ef mononoke root commit root (no-eol)

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport --record-head-symref "$GIT_REPO" --generate-bookmarks full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 2 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 3 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 4 of 5 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 5 of 5 - Oid:* => Bid:* (glob)
  * Ref: "refs/heads/branch1": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/heads/branch2": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(375ef2c64bcda29f59e557d6da26baca67af93b6da5702fcaa2bb626aa1a45e7))) (glob)
  * Ref: "refs/heads/root": Some(ChangesetId(Blake2(*))) (glob)
  * Initializing repo: repo (glob)
  * Initialized repo: repo (glob)
  * All repos initialized. It took: 0 seconds (glob)
  * Bookmark: "heads/branch1": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "heads/branch2": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "heads/master": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "heads/root": ChangesetId(Blake2(*)) (created) (glob)


# Regenerate the Git repo out of the Mononoke repo
  $ mononoke_newadmin git-bundle create from-repo -R repo --output-location "$BUNDLE_PATH"
# Ensure that Git considers this a valid bundle
  $ cd $GIT_REPO
  $ git bundle verify $BUNDLE_PATH
  $TESTTMP/repo_bundle.bundle is okay
  The bundle contains these 5 refs:
  * (glob)
  * (glob)
  * (glob)
  * (glob)
  * (glob)
  The bundle records a complete history.

# Create a new empty folder for containing the repo
  $ mkdir $TESTTMP/git_client_repo  
  $ cd "$TESTTMP"
  $ git clone "$BUNDLE_PATH" git_client_repo
  Cloning into 'git_client_repo'...
  $ cd git_client_repo
# Get the repository log and verify if its the same as earlier
  $ git log --pretty=format:"%h %an %s %D"
  6283891 mononoke Merge branches 'branch1' and 'branch2' HEAD -> master, origin/master, origin/HEAD
  161a8cb mononoke Add master 
  bf946c8 mononoke Add branch1 origin/branch1
  933c6d8 mononoke Add branch2 origin/branch2
  d53a2ef mononoke root commit origin/root (no-eol)

# Dump all the known Git objects into a file
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list

# Ensure that there are no differences between the set of objects by diffing both object list files
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list  
