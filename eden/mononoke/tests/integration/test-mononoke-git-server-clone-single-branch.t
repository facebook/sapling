# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests", "unodes", "filenodes", "hgchangesets"]' setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
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
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
Checkout to the previous commit
  $ git checkout HEAD~1 -q
Create commit in detached state so its not tracked by any branch
  $ echo "this is file3" > file3
  $ git add file3
  $ git commit -qam "Add file3"
Create a tag which becomes the only pointer to this commit
  $ git tag -a -m "tag in detached state" detached_tag
  $ git branch detached_branch
Go back to the master branch
  $ git checkout master -q

# Capture all the known Git objects from the repo
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --single-branch -b master
  Cloning into 'repo'...
  $ cd $REPONAME
# Verify that we indeed did not fetch the tag
  $ git show-ref | grep detached_tag
  [1]
# Verify that we only get the objects associated with the master branch without including any extra objects
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list | sed 's/>//g'
  0a1,2
   006b4ee2e3b26ddd88879444bccb3da30379ed5f tag detached_tag
   20e385682f353d25987c7983b301e5413bbbb645 tree 
  4a7
   a309e46e332a0f166453c6137344852fab38d120 blob file3
  5a9
   d9dc1768c477b85bd1d8bd2d238f234cfe8fbdc4 commit 
