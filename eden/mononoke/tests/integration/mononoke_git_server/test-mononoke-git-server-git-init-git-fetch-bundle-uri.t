# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ cat >> repo_definitions/repo/server.toml <<EOF
  > enable_git_bundle_uri=true
  > EOF
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
  $ git clone "$GIT_REPO_ORIGIN" "$GIT_REPO"
  Cloning into '$TESTTMP/repo-git'...
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

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Put the metadata about the bundle in the DB
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO git_bundles (repo_id, bundle_handle, bundle_list, in_bundle_list_order, bundle_fingerprint, generation_start_timestamp) VALUES (0, '$BUNDLE_PATH', 1, 1, 'bundle_fingerprint', 0)"

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from git_bundles"
  1|0|$TESTTMP/repo_bundle.bundle|1|1|bundle_fingerprint|0

# Git init the repo and git fetch it with bundles
  $ git_client init $REPONAME
  Initialized empty Git repository in $TESTTMP/repo/.git/
  $ echo $REPONAME
  repo
  $ cd $REPONAME
  $ git remote add mononoke $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git

# Show git fetch does use bundles with the fetch.bundleURI config when this is the first fetch.
# Stub the bundle-list in a file
  $ cat << EOF > $TESTTMP/bundle_list_file
  > [bundle]
  > 	version = 1
  > 	mode = all
  > 	heuristic = creationToken
  > [bundle "bundle-1"]
  > 	uri = file://$BUNDLE_PATH
  > 	creationtoken = 1
  > EOF

  $ cat $TESTTMP/bundle_list_file
  [bundle]
  	version = 1
  	mode = all
  	heuristic = creationToken
  [bundle "bundle-1"]
  	uri = file://$TESTTMP/repo_bundle.bundle
  	creationtoken = 1

  $ cd "$TESTTMP/$REPONAME"
  $ GIT_TRACE2=1 GIT_TRACE2_PERF=1 GIT_TRACE_PROTOCOL=1 GIT_TRACE_PACKET=1 git_client -c fetch.bundleURI="file://$TESTTMP/bundle_list_file" -c transfer.bundleURI=true fetch 2>&1 | grep -Eo '(packet:.+(clone|bundle|fetch)|(error|warning)).*$' 
  packet:          git< fetch=shallow wait-for-done filter
  packet:          git< bundle-uri
  packet:        fetch< version 2
  packet:        fetch< ls-refs=unborn
  packet:        fetch< fetch=shallow wait-for-done filter
  packet:        fetch< ref-in-want
  packet:        fetch< object-format=sha1
  packet:        fetch< bundle-uri
  packet:        fetch< 0000
  packet:        fetch> command=ls-refs
  packet:        fetch> object-format=sha1
  packet:        fetch> 0001
  packet:        fetch> peel
  packet:        fetch> symrefs
  packet:        fetch> unborn
  packet:        fetch> ref-prefix refs/heads/
  packet:        fetch> ref-prefix refs/tags/
  packet:        fetch> 0000
  packet:        fetch< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 HEAD symref-target:refs/heads/master_bookmark
  packet:        fetch< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master_bookmark
  packet:        fetch< fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag peeled:e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        fetch< 8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag peeled:8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  packet:        fetch< 0000
  packet:        fetch< 0002


# Show refs
  $ git show-ref
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/bundles/master_bookmark
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/mononoke/master_bookmark
  fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag

# Check we have set the creation token
  $ git config fetch.bundlecreationtoken
  1

# Verify that we get the same Git repo back that we started with
  $ cd "$TESTTMP/$REPONAME"
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list


# Remove the bundle file so any other attempt to use it will fail.
  $ rm $BUNDLE_PATH

# Show git fetch does not try to download bundles again with fetch.bundleURI config.
  $ GIT_TRACE_PROTOCOL=1 GIT_TRACE_PACKET=1 git_client -c fetch.bundleURI="file://$TESTTMP/bundle_list_file" -c transfer.bundleURI=true fetch 2>&1 | grep -Eo '(packet:.+(clone|bundle|fetch)|(error|warning)).*$'
  packet:          git< fetch=shallow wait-for-done filter
  packet:          git< bundle-uri
  packet:        fetch< version 2
  packet:        fetch< ls-refs=unborn
  packet:        fetch< fetch=shallow wait-for-done filter
  packet:        fetch< ref-in-want
  packet:        fetch< object-format=sha1
  packet:        fetch< bundle-uri
  packet:        fetch< 0000
  packet:        fetch> command=ls-refs
  packet:        fetch> object-format=sha1
  packet:        fetch> 0001
  packet:        fetch> peel
  packet:        fetch> symrefs
  packet:        fetch> unborn
  packet:        fetch> ref-prefix refs/heads/
  packet:        fetch> ref-prefix refs/tags/
  packet:        fetch> 0000
  packet:        fetch< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 HEAD symref-target:refs/heads/master_bookmark
  packet:        fetch< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master_bookmark
  packet:        fetch< fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag peeled:e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        fetch< 8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag peeled:8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  packet:        fetch< 0000
  packet:        fetch< 0002


# Let us make a push so there is a new commit in the repo
  $ cd "$GIT_REPO"
  $ git remote add mononoke $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  $ echo abcd > abcd
  $ git add .
  $ git commit -qam "Add file2"
  $ git_client push mononoke --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     e8615d6..7c91d03  master_bookmark -> master_bookmark

# Wait a bit so the server mores the bookmark
  $ sleep 4

# Show git fetch does not try to download bundles again with fetch.bundleURI config even when
# there are new commits available.
  $ cd "$TESTTMP/$REPONAME"
  $ GIT_TRACE_PROTOCOL=1 GIT_TRACE_PACKET=1 git_client -c fetch.bundleURI="file://$TESTTMP/bundle_list_file" -c transfer.bundleURI=true fetch 2>&1 | grep -Eo '(packet:.+(clone|bundle|fetch)|(error|warning)).*$'
  packet:          git< fetch=shallow wait-for-done filter
  packet:          git< bundle-uri
  packet:        fetch< version 2
  packet:        fetch< ls-refs=unborn
  packet:        fetch< fetch=shallow wait-for-done filter
  packet:        fetch< ref-in-want
  packet:        fetch< object-format=sha1
  packet:        fetch< bundle-uri
  packet:        fetch< 0000
  packet:        fetch> command=ls-refs
  packet:        fetch> object-format=sha1
  packet:        fetch> 0001
  packet:        fetch> peel
  packet:        fetch> symrefs
  packet:        fetch> unborn
  packet:        fetch> ref-prefix refs/heads/
  packet:        fetch> ref-prefix refs/tags/
  packet:        fetch> 0000
  packet:        fetch< 7c91d03d49849309acaf941ece272619e246b922 HEAD symref-target:refs/heads/master_bookmark
  packet:        fetch< 7c91d03d49849309acaf941ece272619e246b922 refs/heads/master_bookmark
  packet:        fetch< fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag peeled:e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        fetch< 8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag peeled:8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  packet:        fetch< 0000
  packet:        fetch< 0002
  packet:        fetch> command=fetch
  packet:        fetch> object-format=sha1
  packet:        fetch> 0001
  packet:        fetch> thin-pack
  packet:        fetch> no-progress
  packet:        fetch> include-tag
  packet:        fetch> ofs-delta
  packet:        fetch> want 7c91d03d49849309acaf941ece272619e246b922
  packet:        fetch> have e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        fetch> 0000
  packet:          git< command=fetch
  packet:        fetch< acknowledgments
  packet:        fetch< ACK e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        fetch< 0000
  packet:        fetch< 0002
  packet:        fetch> command=fetch
  packet:        fetch> object-format=sha1
  packet:        fetch> 0001
  packet:        fetch> thin-pack
  packet:        fetch> no-progress
  packet:        fetch> include-tag
  packet:        fetch> ofs-delta
  packet:        fetch> want 7c91d03d49849309acaf941ece272619e246b922
  packet:        fetch> have e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        fetch> done
  packet:        fetch> 0000
  packet:          git< command=fetch
  packet:        fetch< packfile
  packet:        fetch< 0002
