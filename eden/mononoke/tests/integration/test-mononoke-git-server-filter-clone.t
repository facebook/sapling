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
  $ echo "this is file1" > file1
  $ quiet dd if=/dev/zero of=large_file bs=1M count=10
  $ git add .
  $ git commit -qam "Added file1 and large_file"
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ mkdir -p d1/d2/d3/d4/d5 && echo "this is file that is 5 levels deep" > d1/d2/d3/d4/d5/deep_file
  $ git add .
  $ git commit -qam "Add file2 and deep file"
  $ git tag -a empty_tag -m ""
  $ echo "this is modified large file" > large_file
  $ git add .
  $ git commit -qam "Modified large file"
  $ cd "$TESTTMP"
  $ git clone --filter=blob:limit=5k --filter=tree:3 --filter=object:type=blob --filter=object:type=tree --filter=object:type=commit --no-checkout file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --filter=blob:limit=5k --filter=tree:3 --filter=object:type=blob --filter=object:type=tree --filter=object:type=commit 
  Cloning into 'repo'...
  warning: filtering not recognized by server, ignoring
