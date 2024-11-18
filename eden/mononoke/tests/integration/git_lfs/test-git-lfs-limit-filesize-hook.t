# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ GIT_LFS_INTERPRET_POINTERS=1 setup_common_config $REPOTYPE
  $ cat >> repos/repo/server.toml <<EOF
  > [[bookmarks]]
  > regex=".*"
  > hooks_skip_ancestors_of=["heads/master_bookmark"]
  > EOF

  $ register_hook_limit_filesize_global_limit 10 'bypass_pushvar="ALLOW_LARGE_FILES=true"'
  $ testtool_drawdag -R repo << EOF
  > A-B-C
  > # bookmark: C heads/master_bookmark
  > # modify: C large_file regular lfs "contents of LFS file"
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=198d25da38c153f3feecddeee7e49fe3fa16d7e0085ea919c183372bf42a66d4
  $ mononoke_admin derived-data -R repo derive -T git_trees -T git_commits -T git_delta_manifests_v2 -T unodes --all-bookmarks
  $ mononoke_admin git-symref -R repo create --symref-name HEAD --ref-name master_bookmark --ref-type branch
  Symbolic ref HEAD pointing to branch master_bookmark has been added

# Start up the LFS server
  $ LFS_LOG="${TESTTMP}/lfs.log"
  $ LFS_URL="$(lfs_server --log "$LFS_LOG")/repo"

# Start up the Mononoke Git Service
  $ mononoke_git_service --upstream-lfs-server "$LFS_URL/download_sha256"
  $ set_mononoke_as_source_of_truth_for_git

# Clone the Git repo from Mononoke
  $ CLONE_URL="$MONONOKE_GIT_SERVICE_BASE_URL/repo.git"
  $ git_client clone "$CLONE_URL"
  Cloning into 'repo'...
  $ cd repo
  $ git lfs install --local
  Updated ?it hooks. (glob)
  Git LFS initialized.
  $ git config lfs.url "$LFS_URL"
  $ git config http.extraHeader "x-client-info: {\"request_info\": {\"entry_point\": \"CurlTest\", \"correlator\": \"test\"}}"
Try to push a change to non-LFS file
  $ echo contents of LFS file with some extra > some_new_large_file
  $ git add some_new_large_file
  $ git commit -aqm "non-lfs change"
  $ quiet git_client push
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] master_bookmark -> master_bookmark (hooks failed:
    limit_filesize for 423aab438fb40f9f371121bfca1383cd351afe74: File size limit is 10 bytes. You tried to push file some_new_large_file that is over the limit (37 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
  $ git reset --hard origin/master_bookmark
  HEAD is now at 9659866 C

Push a change to LFS file (this should bypass the limit filesize hook)
  $ git lfs track large_file
  Tracking "large_file"
  $ echo contents of LFS file with some extra > large_file
  $ git commit -aqm "new LFS change"
  $ quiet git_client push
  $ mononoke_admin fetch -R repo -B heads/master_bookmark
  BonsaiChangesetId: bc0b66e9dda60bc3c73dc3b56f7a0b65e4eb830e76af6ab595bd5c3759e8983b
  Author: mononoke <mononoke@mononoke>
  Message: new LFS change
  
  FileChanges:
  	 ADDED/MODIFIED (LFS): large_file 408fae710285e464a70ce854d2bdb3d11cba5c9b8d48b135c212c7760681ec31
  
