# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ GIT_LFS_INTERPRET_POINTERS=1 ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests_v2", "unodes", "filenodes", "hgchangesets", "skeleton_manifests"]' setup_common_config $REPOTYPE
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > permit_service_writes = true
  > EOF
  $ testtool_drawdag -R repo << EOF
  > A-B-C
  > # bookmark: C heads/main
  > # modify: C large_file regular lfs "contents of LFS file"
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=198d25da38c153f3feecddeee7e49fe3fa16d7e0085ea919c183372bf42a66d4
  $ mononoke_newadmin derived-data -R repo derive -T git_trees -T git_commits -T git_delta_manifests_v2 -T unodes --all-bookmarks
  $ mononoke_newadmin git-symref -R repo create --symref-name HEAD --ref-name main --ref-type branch
  Symbolic ref HEAD pointing to branch main has been added

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

# List all the objects in Git repo
  $ cd $REPONAME  
  $ cat large_file
  version https://git-lfs.github.com/spec/v1
  oid sha256:f0d0c2c2389643eba52baaa036bf2b66668a996da8c6a1618785ce7f393e46ed
  size 20
  $ git rev-list --objects --all 
  965986666df66943a3496f227d288ae9802102ab
  be393840a21645c52bbde7e62bdb7269fc3ebb87
  8131b4f1da6df2caebe93c581ddd303153b338e5
  463c0410d3fa5af8728525016d18d792fc8c97ea 
  8c7e5a667f1b771847fe88c01c3de34413a1b220 A
  7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54 B
  96d80cd6c4e7158dbebd0849f4fb7ce513e5828c C
  3acb3c7e01e7c40b6e5e154126682bf2d4f43223 large_file
  f6dc85adf6f1fa7fafdd9d57cf66bf6926145bb3 
  617601c79811cbbae338512798318b4e5b70c9ac 

$ cd "$TESTTMP"
  $ git lfs install --local
  Updated ?it hooks. (glob)
  Git LFS initialized.
  $ git config lfs.url "$LFS_URL"
  $ git config http.extraHeader "x-client-info: {\"request_info\": {\"entry_point\": \"CurlTest\", \"correlator\": \"test\"}}"
  $ git_client lfs fetch --all
  fetch: 1 object* found, done. (glob)
  fetch: Fetching all references...
  $ git lfs checkout
  Checking out LFS objects: 100% (1/1), 20 B | 0 B/s, done.
  $ cat large_file
  contents of LFS file (no-eol)

Inspect bonsai for LFS flag
  $ mononoke_newadmin fetch -R repo -B heads/main
  BonsaiChangesetId: 198d25da38c153f3feecddeee7e49fe3fa16d7e0085ea919c183372bf42a66d4
  Author: author
  Message: C
  FileChanges:
  	 ADDED/MODIFIED: C 896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  	 ADDED/MODIFIED (LFS): large_file eb3b8226bb5383aefd8299990543f1f8588344c3b2c2d25182a2a7d1fb691473
  
Push a change to LFS file
  $ git lfs track large_file
  Tracking "large_file"
  $ echo contents of LFS file with some extra > large_file
  $ git commit -aqm "new LFS change"
  $ quiet git_client push
  $ mononoke_newadmin fetch -R repo -B heads/main
  BonsaiChangesetId: bc0b66e9dda60bc3c73dc3b56f7a0b65e4eb830e76af6ab595bd5c3759e8983b
  Author: mononoke <mononoke@mononoke>
  Message: new LFS change
  
  FileChanges:
  	 ADDED/MODIFIED (LFS): large_file 408fae710285e464a70ce854d2bdb3d11cba5c9b8d48b135c212c7760681ec31
  
  $ mononoke_newadmin filestore -R repo fetch  --content-id 5565e648e1bcd80444cedbfb0d86483e2c2ff1b4798d8114454a5de1f25d2248
  version https://git-lfs.github.com/spec/v1
  oid sha256:59c36b4306da9c142ec8feef7bce1964334161db72886faad535f9e2e3418170
  size 37
