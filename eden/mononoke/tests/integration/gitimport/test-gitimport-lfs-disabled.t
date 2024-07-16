# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests", "unodes", "filenodes", "hgchangesets"]' setup_common_config $REPOTYPE
Without that bit gitimport is unable to set bookmarks
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

In this test we're creating another repo that serves only as secondary LFS server - this
way we're showing tha we can deal with the fact that that file contents are uploaded by git
to other LFS server and the import will copy them to Mononoke.
(at Meta this simulates our legacy dewey-lfs setup)
  $ REPOID=2 REPONAME=legacy_lfs setup_common_config $REPOTYPE
  $ cat >> repos/legacy_lfs/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

start LFS server
  $ LFS_LOG="${TESTTMP}/lfs.log"
  $ BASE_LFS_URL="$(lfs_server --log "$LFS_LOG")"
  $ MONONOKE_LFS_URL="$BASE_LFS_URL/repo"
  $ LEGACY_LFS_URL="$BASE_LFS_URL/legacy_lfs"

create a Git repo and one ordinary commit
  $ GIT_REPO_SERVER="${TESTTMP}/repo-git-server"
  $ GIT_REPO_CLIENT="${TESTTMP}/repo-git-client"
  $ git init -q "$GIT_REPO_SERVER" -b main --bare
  $ git clone -q "$GIT_REPO_SERVER" "$GIT_REPO_CLIENT"
  warning: You appear to have cloned an empty repository.
  $ cd "$GIT_REPO_CLIENT"
  $ echo "sml fle" > small_file
  $ git add small_file
  $ git commit -aqm "add small ile"

configure LFS
  $ git lfs install --local
  Updated Git hooks.
  Git LFS initialized.
  $ git config --local lfs.url "$LEGACY_LFS_URL"
  $ git config --local http.extraHeader "x-client-info: {\"request_info\": {\"entry_point\": \"CurlTest\", \"correlator\": \"test\"}}"
  $ git lfs track large_file
  Tracking "large_file"

commit LFS file
  $ echo "laaaaaaaaaarge file" > large_file
  $ git add large_file
  $ git commit -aqm "add large file"
  $ git push -q origin main
  Uploading LFS objects: * (glob)

Git Import
  $ with_stripped_logs gitimport "$GIT_REPO_SERVER" --generate-bookmarks --concurrency 100 --derive-hg full-repo
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/repo-git-server commit 2 of 2 - Oid:ec907399 => Bid:77efb78f
  Hg: Sha1(cd1f06e78e52e64d693fe02d19cf3a427ab1c7f4): HgManifestId(HgNodeHash(Sha1(0ed5ff2a892144296f5abaca61b5759c7f69b551)))
  Hg: Sha1(ec907399950a922e347f484167d9597485acf6a3): HgManifestId(HgNodeHash(Sha1(a754e6297b9438be3c3463bd07f635a7bb26eb39)))
  Ref: "refs/heads/main": Some(ChangesetId(Blake2(77efb78fc3e4ae41846ea7cdcb2cb5a3c65cac8614e841ba31187a97b859f958)))
  Initializing repo: repo
  Initialized repo: repo
  All repos initialized. It took: 0 seconds
  Bookmark: "heads/main": ChangesetId(Blake2(77efb78fc3e4ae41846ea7cdcb2cb5a3c65cac8614e841ba31187a97b859f958)) (created)

We store full file contents for non-LFS file
  $ mononoke_newadmin fetch -R repo -B heads/main --path small_file
  File-Type: regular
  Size: 8
  Content-Id: 5db7cda483f4d35a023d447b8210bd317497193813e9b7ac57268f525277b509
  Sha1: 0e3f29f5c494f653810955ad72d4088f0f62d605
  Sha256: ccaba61b859c0ee7795000dc193cd6db5d0da5a9d13ba1575d9a2fc19d897f85
  Git-Sha1: 8910fc3d7dae273e6ffd1d3982af8dfc418af416
  
  sml fle
  
We store just LFS pointer for LFS file
  $ mononoke_newadmin fetch -R repo -B heads/main --path large_file
  File-Type: regular
  Size: 127
  Content-Id: 46eb1ec21f0a347eb1397b55b6b9bc3cd5a39bf5898728251c25679f987fff57
  Sha1: 28098964e2048ca070d8c2757a4e9c01afb9e41c
  Sha256: e2a71699d1a7ca82bedf1e6bb3dbf2dee6df52915e14dc9570b0d67be5edba0f
  Git-Sha1: 1ab2b3357e304fef596198d92807d8d7e3580f0d
  
  version https://git-lfs.github.com/spec/v1
  oid sha256:6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  size 20
  
This repo has just 2 file content blobs stored
  $ ls "$TESTTMP"/blobstore/blobs/blob-repo0000.content.*
  $TESTTMP/blobstore/blobs/blob-repo0000.content.blake2.46eb1ec21f0a347eb1397b55b6b9bc3cd5a39bf5898728251c25679f987fff57
  $TESTTMP/blobstore/blobs/blob-repo0000.content.blake2.5db7cda483f4d35a023d447b8210bd317497193813e9b7ac57268f525277b509

The actual file content is not uploaded to the repo (this is the hash from pointer)
  $ mononoke_newadmin filestore -R repo fetch  --content-sha256 6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  Error: Content not found
  [1]

But it's available on the separate lfs server
  $ mononoke_newadmin filestore -R legacy_lfs fetch --content-sha256 6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  laaaaaaaaaarge file
