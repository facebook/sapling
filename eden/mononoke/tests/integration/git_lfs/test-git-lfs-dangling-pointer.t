# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-git-lfs.sh"
  $ GIT_LFS_INTERPRET_POINTERS=1 test_repos_for_lfs_with_upstream
  $ testtool_drawdag -R repo << EOF
  > A-B-C
  > # bookmark: C heads/master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_admin derived-data -R repo derive -T git_commits -T git_delta_manifests_v2 -T unodes --all-bookmarks
  $ mononoke_admin git-symref -R repo create --symref-name HEAD --ref-name master_bookmark --ref-type branch
  Symbolic ref HEAD pointing to branch master_bookmark has been added

# Start up the Mononoke Git Service, **intentionally setting up with master_bookmark lfs server as fallback**
  $ mononoke_git_service --upstream-lfs-server "$MONONOKE_LFS_URL/download_sha256"
  $ set_mononoke_as_source_of_truth_for_git

# Clone the Git repo from Mononoke
  $ CLONE_URL="$MONONOKE_GIT_SERVICE_BASE_URL/repo.git"
  $ quiet git_client clone "$CLONE_URL"

# Push with legacy server configures to check if the LFS files end up in mononoke anyway
# The LFS file is uploaded to LEGACY server but the mononoke_git_service won't look there.
  $ cd $REPONAME  
  $ configure_lfs_client_with_legacy_server
  $ echo "contents of LFS file that will be uploaded to legacy server" > large_file
  $ git lfs track large_file
  Tracking "large_file"
  $ git add .gitattributes large_file
  $ git commit -aqm "new LFS change"
  $ quiet git_client push
  Uploading LFS objects: 100% (1/1), 60 B | 0 B/s, done. (?)
  error: remote unpack failed: LFS files missing in Git LFS server. Please upload before pushing. Error:
   find_file_changes
  
  Caused by:
      https://localhost:$LOCAL_PORT/repo/download_sha256/acf1e132a0104ccc9477f3255882466443c5ea4486e9255e994fcd8bf1e0c754 response Response { status: 404, version: HTTP/1.1* (glob)
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (LFS files missing in Git LFS server. Please upload before pushing. Error:
   find_file_changes
  
  Caused by:
      https://localhost:$LOCAL_PORT/repo/download_sha256/acf1e132a0104ccc9477f3255882466443c5ea4486e9255e994fcd8bf1e0c754 response Response { status: 404, version: HTTP/1.1* (glob)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
  $ mononoke_admin fetch -R repo -B heads/master_bookmark
  BonsaiChangesetId: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  Author: author
  Message: C
  FileChanges:
  	 ADDED/MODIFIED: C 896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  
