# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Setup git repository
  $ quiet git_client clone -q $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
# File1 should be large enough that Git create a delta for it
  $ echo "AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1." > file1
  $ git add .
  $ git commit -qam "Add file1"  
# File1 should be large enough that Git create a delta for it
  $ echo "AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile2." > file1
  $ echo "This is file2" > file2.txt
  $ git add .
  $ git commit -qam "Modified file1, Added file2"
# Get list of all objects for verification later
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Push Git repository to Mononoke
  $ git pack-objects --all --stdout > packfile
  $ last_commit=$(git rev-parse HEAD)
# Create the capabilities string
  $ capabilities="report-status quiet object-format=sha1"
  $ printf "00980000000000000000000000000000000000000000 $last_commit refs/heads/master_bookmark\0 $capabilities" >> push_data
  $ echo -n "0000" >> push_data
  $ cat packfile >> push_data
# Pipe the push data to CURL
  $ curl -X POST $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git/git-receive-pack -H 'Content-Type: application/x-git-receive-pack-request' -H 'Accept: application/x-git-receive-pack-result' -H 'Accept-Encoding: deflate, gzip, br' -k --cert "$TEST_CERTDIR/client0.crt" --key "$TEST_CERTDIR/client0.key" --data-binary "@push_data" -s -w "\nResponse code: %{http_code}\n"
  *unpack ok (glob)
  *ok refs/heads/master_bookmark (glob)
  * (glob)
  Response code: 200

  $ wait_for_git_bookmark_create refs/heads/master_bookmark

# Clone the repo from Mononoke and validate that the push worked
  $ cd $TESTTMP
  $ quiet git_client clone -q $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git check_repo
  $ cd check_repo
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list

 
