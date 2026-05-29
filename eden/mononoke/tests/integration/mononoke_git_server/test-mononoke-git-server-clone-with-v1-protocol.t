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

# Enable v1 protocol support
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:git_server_enable_v1_protocol": true
  >   }
  > }
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
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Test ls-remote with v1 protocol — should return all refs
# Verify the correct ref names are present (use awk to extract ref names for stable comparison)
  $ git_client -c protocol.version=1 ls-remote $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git | awk '{print $2}' | sort
  HEAD
  refs/heads/master_bookmark
  refs/remotes/origin/HEAD
  refs/remotes/origin/master_bookmark
  refs/tags/empty_tag
  refs/tags/first_tag

# Delete the HEAD symref to test v1 protocol without HEAD configured
  $ mononoke_admin git-symref -R repo delete --symref-names HEAD
  Successfully deleted symrefs ["HEAD"]

# Test ls-remote with v1 protocol when HEAD symref is missing — should still return all branch/tag refs without error
# HEAD still appears because it exists as a regular bookmark from gitimport, but without the symref capability
  $ git_client -c protocol.version=1 ls-remote $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git | awk '{print $2}' | sort
  HEAD
  refs/heads/master_bookmark
  refs/remotes/origin/HEAD
  refs/remotes/origin/master_bookmark
  refs/tags/empty_tag
  refs/tags/first_tag
