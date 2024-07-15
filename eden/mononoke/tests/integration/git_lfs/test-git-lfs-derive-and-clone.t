# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests", "unodes", "filenodes", "hgchangesets"]' setup_common_config $REPOTYPE
  $ testtool_drawdag -R repo << EOF
  > A-B-C
  > # bookmark: C heads/main
  > # modify: C large_file regular lfs "contents of LFS file"
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=1054de6604c183c5326e56061e05c63922c3c8bd49c4d1d4e51d129ce8fbc7c8
  $ quiet backfill_derived_data backfill-all git_trees git_commits git_delta_manifests unodes
  $ mononoke_newadmin git-symref -R repo create --symref-name HEAD --ref-name main --ref-type branch
  Symbolic ref HEAD pointing to branch main has been added

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Clone the Git repo from Mononoke
  $ CLONE_URL="$MONONOKE_GIT_SERVICE_BASE_URL/repo.git"
  $ git_client clone "$CLONE_URL"
  Cloning into 'repo'...

# List all the objects in Git repo
  $ cd $REPONAME  
  $ cat large_file
  version https://git-lfs.github.com/spec/v1
  oid sha256:f0d0c2c2389643eba52baaa036bf2b66668a996da8c6a1618785ce7f393e46ed
  size 20 (no-eol)
  $ git rev-list --objects --all 
  debb49562a1b388c240ad57b758baa683d5d2fb7
  be393840a21645c52bbde7e62bdb7269fc3ebb87
  8131b4f1da6df2caebe93c581ddd303153b338e5
  61eda68aa48e8a2e5053bcf2c4244c0368173053 
  8c7e5a667f1b771847fe88c01c3de34413a1b220 A
  7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54 B
  96d80cd6c4e7158dbebd0849f4fb7ce513e5828c C
  2adb0e3bdc5bf435a8276d61a50ff7a0b82912fb large_file
  f6dc85adf6f1fa7fafdd9d57cf66bf6926145bb3 
  617601c79811cbbae338512798318b4e5b70c9ac 

$ cd "$TESTTMP"
  $ LFS_LOG="${TESTTMP}/lfs.log"
  $ LFS_URL="$(lfs_server --log "$LFS_LOG")/repo"
  $ git lfs install --local
  Updated Git hooks.
  Git LFS initialized.
  $ git_client -c "lfs.url=$LFS_URL" -c http.extraHeader="x-client-info: {\"request_info\": {\"entry_point\": \"CurlTest\", \"correlator\": \"test\"}}" lfs fetch --all
  fetch: 1 object found, done.
  fetch: Fetching all references...
  $ git lfs checkout
  Checking out LFS objects: 100% (1/1), 20 B | 0 B/s, done.
  $ cat large_file
  contents of LFS file (no-eol)
