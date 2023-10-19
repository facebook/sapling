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

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is a file with UTF-8 content like ॐ" > file1
  $ git add file1
  $ git commit -q -am "मैं परीक्षण कर रहा हूँ"
  $ echo "this is another file with UTF-8 like नमस्ते" > file2
  $ git add file2
  $ git commit -q -am "यह एक और परीक्षा है"
  $ git tag -a empty_tag -m "टैग की गई प्रतिबद्धता"
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
  $ gitimport --record-head-symref "$GIT_REPO" --derive-hg --generate-bookmarks full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 2 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 2 of 2 - Oid:* => Bid:* (glob)
  * Hg: Sha1(*): HgManifestId(HgNodeHash(Sha1(*))) (glob)
  * Hg: Sha1(*): HgManifestId(HgNodeHash(Sha1(*))) (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/remotes/origin/HEAD": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/remotes/origin/master": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/tags/empty_tag": Some(ChangesetId(Blake2(*))) (glob)
  * Initializing repo: repo (glob)
  * Initialized repo: repo (glob)
  * All repos initialized. It took: 0 seconds (glob)
  * Bookmark: "heads/master": ChangesetId(Blake2(*)) (created) (glob)
  * Bookmark: "heads/master": ChangesetId(Blake2(*)) (already up-to-date) (glob)
  * Bookmark: "tags/empty_tag": ChangesetId(Blake2(*)) (created) (glob)

# Regenerate the Git repo out of the Mononoke repo
  $ mononoke_newadmin git-bundle create from-repo -R repo --output-location "$BUNDLE_PATH"

# Ensure that Git considers this a valid bundle
  $ cd $GIT_REPO
  $ git bundle verify $BUNDLE_PATH
  $TESTTMP/repo_bundle.bundle is okay
  The bundle contains these 3 refs:
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
  $ git log --pretty=format:"%h %an %s %D" > $TESTTMP/new_repo_log
  $ diff -w $TESTTMP/new_repo_log $TESTTMP/repo_log

# Print out the log to ensure the commit messages have carried over without data loss
  $ git log --pretty=format:"%h %an %s %D"
  d46416c mononoke यह एक और परीक्षा है HEAD -> master, tag: empty_tag, origin/master, origin/HEAD
  65d5239 mononoke मैं परीक्षण कर रहा हूँ  (no-eol)

# Dump all the known Git objects into a file
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list

# Ensure that there are no differences between the set of objects by diffing both object list files
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list  
