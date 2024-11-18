# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo"
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"

# Setup git repsitory
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ git commit --allow-empty -m "root commit"
  [master_bookmark (root-commit) d53a2ef] root commit
  $ git branch root

  $ echo "this is master_bookmark" > master_bookmark
  $ git add master_bookmark
  $ git commit -qam "Add master_bookmark"

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

  $ git checkout -q master_bookmark
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
  b83b948 mononoke Merge branches 'branch1' and 'branch2' into master_bookmark HEAD -> master_bookmark
  06a9845 mononoke Add master_bookmark 
  bf946c8 mononoke Add branch1 branch1
  933c6d8 mononoke Add branch2 branch2
  d53a2ef mononoke root commit root (no-eol)

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ with_stripped_logs gitimport "$GIT_REPO" --generate-bookmarks full-repo
  using repo "repo" repoid RepositoryId(0)
  GitRepo:*repo-git commit 5 of 5 - Oid:* => Bid:* (glob)
  Ref: "refs/heads/branch1": Some(ChangesetId(Blake2(*))) (glob)
  Ref: "refs/heads/branch2": Some(ChangesetId(Blake2(*))) (glob)
  Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(cd646b3089570264b64aad059cf0420b0564f9a57e3b243921da560d9e94339c)))
  Ref: "refs/heads/root": Some(ChangesetId(Blake2(*))) (glob)
  Initializing repo: repo
  Initialized repo: repo
  All repos initialized. It took: * seconds (glob)
  Bookmark: "heads/branch1": ChangesetId(Blake2(*)) (created) (glob)
  Bookmark: "heads/branch2": ChangesetId(Blake2(*)) (created) (glob)
  Bookmark: "heads/master_bookmark": ChangesetId(Blake2(*)) (created) (glob)
  Bookmark: "heads/root": ChangesetId(Blake2(*)) (created) (glob)


# Regenerate the Git repo out of the Mononoke repo
  $ mononoke_admin git-bundle create from-repo -R repo --output-location "$BUNDLE_PATH"
# Ensure that Git considers this a valid bundle
  $ cd $GIT_REPO
  $ git bundle verify -q $BUNDLE_PATH
  $TESTTMP/repo_bundle.bundle is okay

# Create a new empty folder for containing the repo
  $ mkdir $TESTTMP/git_client_repo  
  $ cd "$TESTTMP"
  $ git clone --mirror "$BUNDLE_PATH" git_client_repo
  Cloning into bare repository 'git_client_repo'...
  $ cd git_client_repo
# Get the repository log and verify if its the same as earlier
  $ git log --pretty=format:"%h %an %s %D"
  b83b948 mononoke Merge branches 'branch1' and 'branch2' into master_bookmark HEAD -> master_bookmark
  06a9845 mononoke Add master_bookmark 
  bf946c8 mononoke Add branch1 branch1
  933c6d8 mononoke Add branch2 branch2
  d53a2ef mononoke root commit root (no-eol)

# Dump all the known Git objects into a file
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list

# Ensure that there are no differences between the set of objects by diffing both object list files
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list  
