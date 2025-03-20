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
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"

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

# Create a bundle on disk
  $ mononoke_admin git-bundle create from-repo -R repo --output-location "$BUNDLE_PATH"

# Enable bundle-uri capability for the repo
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:git_bundle_uri_capability": true
  >   }
  > }
  > EOF

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Put the metadata about the bundle in the DB
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO git_bundles (repo_id, bundle_handle, bundle_list, in_bundle_list_order, bundle_fingerprint) VALUES (0, '$BUNDLE_PATH', 1, 1, 'bundle_fingerprint')"

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from git_bundles"
  1|0|$TESTTMP/repo_bundle.bundle|1|1|bundle_fingerprint

# Clone the Git repo from Mononoke
  $ GIT_TRACE_PACKET=1 git_client -c transfer.bundleURI=true clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git 2>&1 | grep -oE 'packet:.+bundle.*$'
  packet:          git< bundle-uri
  packet:        clone< bundle-uri
  packet:        clone> command=bundle-uri
  packet:          git< command=bundle-uri
  packet:        clone< bundle.version=1
  packet:        clone< bundle.mode=all
  packet:        clone< bundle.bundle_bundle_fingerprint.uri=file://$TESTTMP/repo_bundle.bundle

# Verify that we get the same Git repo back that we started with
  $ cd $REPONAME  
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list  
