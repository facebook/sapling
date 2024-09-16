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
  > # bookmark: C heads/main
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo derive -T git_trees -T git_commits -T git_delta_manifests_v2 -T unodes --all-bookmarks
  $ mononoke_newadmin git-symref -R repo create --symref-name HEAD --ref-name main --ref-type branch
  Symbolic ref HEAD pointing to branch main has been added

# Start up the Mononoke Git Service, **intentionally setting up with main lfs server as fallback**
  $ mononoke_git_service --upstream-lfs-server "$MONONOKE_LFS_URL/download_sha256"
  $ set_mononoke_as_source_of_truth_for_git

# Clone the Git repo from Mononoke
  $ CLONE_URL="$MONONOKE_GIT_SERVICE_BASE_URL/repo.git"
  $ git_client clone "$CLONE_URL"
  Cloning into 'repo'...

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
  Uploading LFS objects: 100% (1/1), 60 B | 0 B/s, done.
  error: RPC failed; HTTP 500 curl 22 The requested URL returned error: 500
  fatal: the remote end hung up unexpectedly
  Everything up-to-date
  [1]
  $ mononoke_newadmin fetch -R repo -B heads/main
  BonsaiChangesetId: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  Author: author
  Message: C
  FileChanges:
  	 ADDED/MODIFIED: C 896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  
