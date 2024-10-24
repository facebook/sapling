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
  $ mononoke_newadmin derived-data -R repo derive -T git_trees -T git_commits -T git_delta_manifests_v2 -T unodes --all-bookmarks
  $ mononoke_newadmin git-symref -R repo create --symref-name HEAD --ref-name master_bookmark --ref-type branch
  Symbolic ref HEAD pointing to branch master_bookmark has been added

# Setup hooks
  $ cat >> "${TESTTMP}/mononoke-config/repos/repo/server.toml" <<EOF
  > [[bookmarks]]
  > regex=".*"
  > [[bookmarks.hooks]]
  > hook_name="missing_lfsconfig"
  > [[hooks]]
  > name="missing_lfsconfig"
  > config_json='{}'
  > EOF

# Start up the Mononoke Git Service
  $ mononoke_git_service --upstream-lfs-server "$LEGACY_LFS_URL/download_sha256"
  $ set_mononoke_as_source_of_truth_for_git

# Clone the Git repo from Mononoke
  $ CLONE_URL="$MONONOKE_GIT_SERVICE_BASE_URL/repo.git"
  $ git_client clone "$CLONE_URL"
  Cloning into 'repo'...

# Push without .lfsconfig in the repo. This should fail.
  $ cd $REPONAME  
  $ configure_lfs_client_with_legacy_server
  $ echo "contents of LFS file that will be uploaded to legacy server" > large_file
  $ git lfs track large_file
  Tracking "large_file"
  $ git add .gitattributes large_file
  $ git commit -aqm "new LFS change"
  $ quiet git_client push origin master_bookmark
  Uploading LFS objects: 100% (1/1), 60 B | 0 B/s, done. (?)
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    missing_lfsconfig for 1fc5ba8b03de67a5e0eeb7e61c0df0703e3715e8: You need to add a properly defined .lfsconfig at the root directory of the repository prior to pushing.
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# Add lfsconfig file and push. This should succeed.
  $ echo "[section]"> .lfsconfig
  $ git add .lfsconfig
  $ git commit --amend -aqm "new LFS change with .lfsconfig"
  $ quiet git_client push origin master_bookmark
  $ mononoke_newadmin fetch -R repo -B heads/master_bookmark
  BonsaiChangesetId: f43135737efc212651cdf8ab6fadb217305ac4d274e269673522198f6b57b53b
  Author: mononoke <mononoke@mononoke>
  Message: new LFS change with .lfsconfig
  
  FileChanges:
  	 ADDED/MODIFIED: .gitattributes 9c803b34f20a6e774db43175832c29c0ec5bc08ab09329f63c619cb03a6ebb7b
  	 ADDED/MODIFIED: .lfsconfig b6d78c0e31f537cc0367b8be3505d609a0dffd4713024110b8d37dade4321c10
  	 ADDED/MODIFIED (LFS): large_file 978e55f6ff83794e598f13fb0f4f30bca32dd1dda8b57df5983a4dba00cc7ef2
  
  $ mononoke_newadmin filestore -R repo fetch  --content-id 978e55f6ff83794e598f13fb0f4f30bca32dd1dda8b57df5983a4dba00cc7ef2
  contents of LFS file that will be uploaded to legacy server

# Add commit with regular file
  $ echo file > new_file
  $ git add new_file
  $ git commit -aqm "new commit with regular file"
  $ git_client push origin master_bookmark
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     c665d3d..02afe80  master_bookmark -> master_bookmark

# Add another LFS file
  $ echo "contents of another LFS file that will be uploaded to legacy server" > another_large_file
  $ git lfs track another_large_file
  Tracking "another_large_file"
  $ git add another_large_file
  $ git commit -aqm "new commit with another large file"
  $ git_client push origin master_bookmark
  Uploading LFS objects: 100% (1/1), 68 B | 0 B/s, done. (?)
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     02afe80..72162d7  master_bookmark -> master_bookmark

# Add a regular commit to the repo but through a new branch
  $ git checkout -b regular_branch
  Switched to a new branch 'regular_branch'
  $ echo "Just a regular file" > file.txt
  $ git add .
  $ git commit -qam "Just a regular commit"
  $ git_client push origin HEAD:refs/heads/regular_branch
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new branch]      HEAD -> regular_branch
