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
  contents of LFS file (no-eol)
  $ git rev-list --objects --all 
  573aa48fcae9dbe43b1cdb399e5347679b9826fa
  be393840a21645c52bbde7e62bdb7269fc3ebb87
  8131b4f1da6df2caebe93c581ddd303153b338e5
  e96054e75b8a16ac4fdd06b86da5a42d4aadcddd 
  8c7e5a667f1b771847fe88c01c3de34413a1b220 A
  7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54 B
  96d80cd6c4e7158dbebd0849f4fb7ce513e5828c C
  3222f7f375a57003fc49c796a44c103701106139 large_file
  f6dc85adf6f1fa7fafdd9d57cf66bf6926145bb3 
  617601c79811cbbae338512798318b4e5b70c9ac 

$ cd "$TESTTMP"
  $ LFS_LOG="${TESTTMP}/lfs.log"
  $ LFS_URL="$(lfs_server --log "$LFS_LOG")/repo"
  $ git lfs install --local
  Updated Git hooks.
  Git LFS initialized.
  $ git_client -c "lfs.url=$LFS_URL" -c http.extraHeader="x-client-info: {\"request_info\": {\"entry_point\": \"CurlTest\", \"correlator\": \"test\"}}" lfs fetch --all
  fetch: Fetching all references...
  $ git lfs checkout
  $ cat large_file
  contents of LFS file (no-eol)
