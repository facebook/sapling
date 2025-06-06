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

# Clone the Git repo from Mononoke. Git:
# 1. Asks the server which refs does it have (command=ls-refs)
# 2. Fetches the bundle-list. (command=bundle-uri)
# 3. Fetches the bundle from the bundle-list. (not shown here)
# 4. Does incremental fetch indicating it has got master_bookmark from the server (clone> have e8615d6f149b876be0a2f30a1c5bf0c42bf8e136)
  $ GIT_TRACE_PACKET=1 git_client -c transfer.bundleURI=true clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git $REPONAME 2>&1 | grep -Eo '(packet:.+(clone|bundle)|(error|warning)).*$' 
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
  packet:        clone< bundle.heuristic=creationToken
  packet:        clone< bundle.bundle_bundle_fingerprint.uri=file://$TESTTMP/repo_bundle.bundle
  packet:        clone< bundle.bundle_bundle_fingerprint.creationtoken=1
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

# Check we have set the creation token
  $ git config fetch.bundlecreationtoken
  1

# Show refs
  $ git show-ref
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/bundles/master_bookmark
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master_bookmark
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/HEAD
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/master_bookmark
  fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag

# Remove the bundle file so any other attempt to use it will fail.
  $ rm $TESTTMP/repo_bundle.bundle

# Show git fetch does not download bundles again without the fetch.bundleURI config.
  $ GIT_TRACE_PROTOCOL=1 GIT_TRACE_PACKET=1 git_client -c transfer.bundleURI=true fetch 2>&1 | grep -Eo '(packet:.+(clone|bundle|fetch)|(error|warning)).*$' 
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
  packet:        fetch> ref-prefix refs/heads/master_bookmark
  packet:        fetch> ref-prefix refs/tags/
  packet:        fetch> 0000
  packet:        fetch< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 HEAD symref-target:refs/heads/master_bookmark
  packet:        fetch< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master_bookmark
  packet:        fetch< fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag peeled:e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        fetch< 8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag peeled:8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  packet:        fetch< 0000
  packet:        fetch< 0002


# Show git fetch does not download bundles again with the fetch.bundleURI config.
# Stub the bundle-list in a file
  $ cat << EOF > $TESTTMP/bundle_list_file
  > [bundle]
  > 	version = 1
  > 	mode = all
  > 	heuristic = creationToken
  > [bundle "bundle-1"]
  > 	uri = file://NONEXISTENT
  > 	creationtoken = 1
  > EOF

  $ cat $TESTTMP/bundle_list_file
  [bundle]
  	version = 1
  	mode = all
  	heuristic = creationToken
  [bundle "bundle-1"]
  	uri = file://NONEXISTENT
  	creationtoken = 1

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
  packet:        fetch> ref-prefix refs/heads/master_bookmark
  packet:        fetch> ref-prefix refs/tags/
  packet:        fetch> 0000
  packet:        fetch< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 HEAD symref-target:refs/heads/master_bookmark
  packet:        fetch< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master_bookmark
  packet:        fetch< fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag peeled:e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        fetch< 8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag peeled:8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  packet:        fetch< 0000
  packet:        fetch< 0002

# Show git fetch does try to download bundles again with fetch.bundleURI config when the creationtoken is larger than the one currently set
  $ cat << EOF > $TESTTMP/bundle_list_file
  > [bundle]
  > 	version = 1
  > 	mode = all
  > 	heuristic = creationToken
  > [bundle "bundle-1"]
  > 	uri = file://NONEXISTENT
  > 	creationtoken = 2
  > EOF
  $ cat $TESTTMP/bundle_list_file
  [bundle]
  	version = 1
  	mode = all
  	heuristic = creationToken
  [bundle "bundle-1"]
  	uri = file://NONEXISTENT
  	creationtoken = 2
  $ GIT_TRACE_PROTOCOL=1 GIT_TRACE_PACKET=1 git_client -c fetch.bundleURI="file://$TESTTMP/bundle_list_file" -c transfer.bundleURI=true fetch 2>&1 | grep -Eo '(packet:.+(clone|bundle|fetch)|(error|warning)).*$' 
  warning: failed to download bundle from URI 'file://NONEXISTENT'
  warning: file at URI 'file://$TESTTMP/bundle_list_file' is not a bundle or bundle list
  warning: failed to fetch bundles from 'file://$TESTTMP/bundle_list_file'
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
  packet:        fetch> ref-prefix refs/heads/master_bookmark
  packet:        fetch> ref-prefix refs/tags/
  packet:        fetch> 0000
  packet:        fetch< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 HEAD symref-target:refs/heads/master_bookmark
  packet:        fetch< e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master_bookmark
  packet:        fetch< fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag peeled:e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        fetch< 8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag peeled:8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  packet:        fetch< 0000
  packet:        fetch< 0002

# Show the repo
  $ git log --oneline --graph --all
  * e8615d6 Add file2
  * 8ce3eae Add file1

# Let us make a push so there is a new commit in the repo
  $ cd "$GIT_REPO"
  $ git remote add mononoke $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  $ echo abcd > abcd
  $ git add .
  $ git commit -qam "Add file2"
  $ git_client push mononoke --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     e8615d6..7c91d03  master_bookmark -> master_bookmark

# Show the repo
  $ git log --oneline --graph --all
  * 7c91d03 Add file2
  * e8615d6 Add file2
  * 8ce3eae Add file1

# Wait a bit for the master_bookmark to move in mononoke
  $ sleep 4

# Show git fetch does not download bundles again without the fetch.bundleURI config even after new commits available.
  $ cd "$TESTTMP/$REPONAME"
  $ GIT_TRACE2_PERF=1 GIT_TRACE_PROTOCOL=1 GIT_TRACE_PACKET=1 git_client -c transfer.bundleURI=true fetch 2>&1 | grep -Eo '(packet:.+(clone|bundle|fetch)|(error|warning)).*$' 
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
  packet:        fetch> ref-prefix refs/heads/master_bookmark
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

# Show the repo
  $ git log --oneline --graph --all
  * 7c91d03 Add file2
  * e8615d6 Add file2
  * 8ce3eae Add file1

# Let us make a push so there is a new commit in the repo
  $ cd "$GIT_REPO"
  $ echo abcde > abcde
  $ git add .
  $ git commit -qam "Add file2"
  $ git_client push mononoke --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     7c91d03..4f541ac  master_bookmark -> master_bookmark

# Wait a bit for the master_bookmark to move in mononoke
  $ sleep 4

# Show git fetch does not download bundles again with the fetch.bundleURI config even after new commits available.
# Stub the bundle-list in a file
  $ cd "$TESTTMP/$REPONAME"
  $ cat << EOF > $TESTTMP/bundle_list_file
  > [bundle]
  > 	version = 1
  > 	mode = all
  > 	heuristic = creationToken
  > [bundle "bundle-1"]
  > 	uri = file://NONEXISTENT
  > 	creationtoken = 1
  > EOF

  $ cat $TESTTMP/bundle_list_file
  [bundle]
  	version = 1
  	mode = all
  	heuristic = creationToken
  [bundle "bundle-1"]
  	uri = file://NONEXISTENT
  	creationtoken = 1

  $ GIT_TRACE2_PERF=1 GIT_TRACE_PROTOCOL=1 GIT_TRACE_PACKET=1 git_client -c fetch.bundleURI="file://$TESTTMP/bundle_list_file" -c transfer.bundleURI=true fetch 2>&1 | grep -Eo '(packet:.+(clone|bundle|fetch)|(error|warning)).*$' 
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
  packet:        fetch> ref-prefix refs/heads/master_bookmark
  packet:        fetch> ref-prefix refs/tags/
  packet:        fetch> 0000
  packet:        fetch< 4f541acd9f7598f86f96b444b9040a83cdda6456 HEAD symref-target:refs/heads/master_bookmark
  packet:        fetch< 4f541acd9f7598f86f96b444b9040a83cdda6456 refs/heads/master_bookmark
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
  packet:        fetch> want 4f541acd9f7598f86f96b444b9040a83cdda6456
  packet:        fetch> have e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  packet:        fetch> have 7c91d03d49849309acaf941ece272619e246b922
  packet:        fetch> 0000
  packet:          git< command=fetch
  packet:        fetch< acknowledgments
  packet:        fetch< ACK (7c91d03d49849309acaf941ece272619e246b922|e8615d6f149b876be0a2f30a1c5bf0c42bf8e136) (re)
  packet:        fetch< ACK (7c91d03d49849309acaf941ece272619e246b922|e8615d6f149b876be0a2f30a1c5bf0c42bf8e136) (re)
  packet:        fetch< 0000
  packet:        fetch< 0002
  packet:        fetch> command=fetch
  packet:        fetch> object-format=sha1
  packet:        fetch> 0001
  packet:        fetch> thin-pack
  packet:        fetch> no-progress
  packet:        fetch> include-tag
  packet:        fetch> ofs-delta
  packet:        fetch> (want 4f541acd9f7598f86f96b444b9040a83cdda6456|have 7c91d03d49849309acaf941ece272619e246b922|have e8615d6f149b876be0a2f30a1c5bf0c42bf8e136) (re)
  packet:        fetch> (want 4f541acd9f7598f86f96b444b9040a83cdda6456|have 7c91d03d49849309acaf941ece272619e246b922|have e8615d6f149b876be0a2f30a1c5bf0c42bf8e136) (re)
  packet:        fetch> (want 4f541acd9f7598f86f96b444b9040a83cdda6456|have 7c91d03d49849309acaf941ece272619e246b922|have e8615d6f149b876be0a2f30a1c5bf0c42bf8e136) (re)
  packet:        fetch> done
  packet:        fetch> 0000
  packet:          git< command=fetch
  packet:        fetch< packfile
  packet:        fetch< 0002
