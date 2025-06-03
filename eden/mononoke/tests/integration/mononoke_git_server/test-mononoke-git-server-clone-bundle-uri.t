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
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO git_bundles (repo_id, bundle_handle, bundle_list, in_bundle_list_order, bundle_fingerprint, generation_start_timestamp) VALUES (0, '$BUNDLE_PATH', 1, 1, 'bundle_fingerprint', 0)"

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from git_bundles"
  1|0|$TESTTMP/repo_bundle.bundle|1|1|bundle_fingerprint|0

# Clone the Git repo from Mononoke. Git:
# 1. Asks the server which refs does it have (command=ls-refs)
# 2. Fetches the bundle-list. (command=bundle-uri)
# 3. Fetches the bundle from the bundle-list. (not shown here)
# 4. Does incremental fetch indicating it has got master_bookmark from the server (clone> have e8615d6f149b876be0a2f30a1c5bf0c42bf8e136)
  $ GIT_TRACE2_PERF=1 GIT_TRACE_PACKET=1 git_client -c transfer.bundleURI=true clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git 2>&1 | grep -Eo 'packet:.+(clone|bundle).*$'
  packet:          git< bundle-uri
  packet:        clone< version 2
  packet:        clone< ls-refs=unborn
  packet:        clone< fetch=shallow wait-for-done filter
  packet:        clone< ref-in-want
  packet:        clone< object-format=sha1
  packet:        clone< bundle-uri
  packet:        clone< 0000
  packet:        clone> command=ls-refs
  packet:        clone> object-format=sha1
  packet:        clone> 0001
  packet:        clone> peel
  packet:        clone> symrefs
  packet:        clone> unborn
  packet:        clone> ref-prefix HEAD
  packet:        clone> ref-prefix refs/heads/
  packet:        clone> ref-prefix refs/tags/
  packet:        clone> 0000
  packet:        clone< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 HEAD symref-target:refs/heads/master_bookmark
  packet:        clone< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master_bookmark
  packet:        clone< fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag peeled:e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        clone< 8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag peeled:8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  packet:        clone< 0000
  packet:        clone< 0002
  packet:        clone> object-format=sha1
  packet:        clone> command=bundle-uri
  packet:        clone> 0001
  packet:        clone> 0000
  packet:          git< command=bundle-uri
  packet:        clone< bundle.version=1
  packet:        clone< bundle.mode=all
  packet:        clone< bundle.bundle_bundle_fingerprint.uri=file://$TESTTMP/repo_bundle.bundle
  packet:        clone< 0000
  packet:        clone< 0002
  packet:        clone> command=fetch
  packet:        clone> object-format=sha1
  packet:        clone> 0001
  packet:        clone> thin-pack
  packet:        clone> no-progress
  packet:        clone> ofs-delta
  packet:        clone> want fb02ed046a1e75fe2abb8763f7c715496ae36353
  packet:        clone> want 8963e1f55d1346a07c3aec8c8fc72bf87d0452b1
  packet:        clone> have e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        clone> 0000
  packet:        clone< acknowledgments
  packet:        clone< ACK e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        clone< 0000
  packet:        clone< 0002
  packet:        clone> command=fetch
  packet:        clone> object-format=sha1
  packet:        clone> 0001
  packet:        clone> thin-pack
  packet:        clone> no-progress
  packet:        clone> ofs-delta
  packet:        clone> want fb02ed046a1e75fe2abb8763f7c715496ae36353
  packet:        clone> want 8963e1f55d1346a07c3aec8c8fc72bf87d0452b1
  packet:        clone> have e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        clone> done
  packet:        clone> 0000
  packet:        clone< packfile
  packet:        clone< 0002

# Verify that we get the same Git repo back that we started with
  $ cd $REPONAME  
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list  
