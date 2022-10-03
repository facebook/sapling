# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hg init repo-hg --config format.usefncache=False

  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > commitextras=
  > treemanifest=!
  > treemanifestserver=
  > [treemanifest]
  > server=True
  > [workingcopy]
  > ruststatus=False
  > EOF

  $ touch file1
  $ hg add
  adding file1
  $ hg commit -m "adding first commit"

  $ touch file2
  $ hg add
  adding file2
  $ hg commit -m "adding second commit"

Set up the base repo, and a fake source repo
  $ setup_mononoke_config
  $ REPOID=65535 REPONAME=megarepo setup_common_config "blob_files"
  $ setup_configerator_configs
  $ cd $TESTTMP

Do the import, cross-check that the mapping is preserved
  $ blobimport repo-hg/.hg repo --source-repo-id 65535
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select large_repo_id, hex(large_bcs_id), small_repo_id, hex(small_bcs_id), source_repo from synced_commit_mapping;" | sort
  0|59695D47BD01807288E7A7D14AAE5E93507C8B4E2B48B8CC4947B18C0E8BF471|65535|59695D47BD01807288E7A7D14AAE5E93507C8B4E2B48B8CC4947B18C0E8BF471|small
  0|73D11CCF7D3515BFD96DC1F43FF5A2E51636F4D9ECC299E3246C7C46ED55E874|65535|73D11CCF7D3515BFD96DC1F43FF5A2E51636F4D9ECC299E3246C7C46ED55E874|small
