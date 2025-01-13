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
  $ git_client clone -q $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git "$GIT_REPO_ORIGIN"
  warning: You appear to have cloned an empty repository.
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
# File1 should be large enough that Git create a delta for it
  $ echo "AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1." > file1
  $ git add .
  $ git commit -qam "Add file1"  
  $ git tag -a -m "new tag" first_tag  
# File1 should be large enough that Git create a delta for it
  $ echo "AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile1.AddFile2." > file1
  $ echo "This is file2" > file2.txt
  $ git add .
  $ git commit -qam "Modified file1, Added file2"
  $ git tag -a empty_tag -m ""

Push Git repository to Mononoke
  $ git pack-objects --all --stdout > packfile
  $ last_commit=$(git rev-parse HEAD)
Create the capabilities string
  $ capabilities="report-status quiet object-format=sha1"
  $ printf "00980000000000000000000000000000000000000000 $last_commit refs/heads/master_bookmark\0 $capabilities" >> push_data
  $ echo -n "0000" >> push_data
  $ cat packfile >> push_data
Pipe the push data to CURL
  $ curl -X POST $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git/git-receive-pack -H 'Content-Type: application/x-git-receive-pack-request' -H 'Accept: application/x-git-receive-pack-result' -H 'Accept-Encoding: deflate, gzip, br' -k --cert "$TEST_CERTDIR/client0.crt" --key "$TEST_CERTDIR/client0.key" --data-binary "@push_data" -s -w "\nResponse code: %{http_code}\n"
  Error in decoding packfile entry: RefDelta { base_id: Sha1(ccce194714ee3fae382fad2f4a67cd6c1a7320cd) }: A delta chain could not be followed as the ref base with id ccce194714ee3fae382fad2f4a67cd6c1a7320cd could not be found
  Response code: 500

 
