# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-git-lfs.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests_v2", "unodes", "filenodes", "hgchangesets"]' setup_common_config $REPOTYPE
Without that bit gitimport is unable to set bookmarks
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

Use common repo setup
  $ test_repos_for_git_lfs_import

Git Import
  $ with_stripped_logs gitimport "$GIT_REPO_SERVER" --generate-bookmarks --concurrency 100 --derive-hg full-repo
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/repo-git-server commit 3 of 3 - Oid:c13a0ad2 => Bid:63ca8c6f
  Hg: Sha1(cd1f06e78e52e64d693fe02d19cf3a427ab1c7f4): HgManifestId(HgNodeHash(Sha1(0ed5ff2a892144296f5abaca61b5759c7f69b551)))
  Hg: Sha1(ec907399950a922e347f484167d9597485acf6a3): HgManifestId(HgNodeHash(Sha1(a754e6297b9438be3c3463bd07f635a7bb26eb39)))
  Hg: Sha1(c13a0ad234813977286c5827533de22af7f04fc5): HgManifestId(HgNodeHash(Sha1(8c3afe88bfee82fe7eaa26c061875ce6395f9a98)))
  Ref: "refs/heads/main": Some(ChangesetId(Blake2(63ca8c6ff5810be0626a3d9d84f08e39ff4236b6e9907cc2aeaaba73d520a0c7)))
  Initializing repo: repo
  Initialized repo: repo
  All repos initialized. It took: * seconds (glob)
  Bookmark: "heads/main": ChangesetId(Blake2(63ca8c6ff5810be0626a3d9d84f08e39ff4236b6e9907cc2aeaaba73d520a0c7)) (created)

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
  

  $ mononoke_newadmin fetch -R repo -B heads/main --path large_file_non_canonical_pointer
  File-Type: regular
  Size: 126
  Content-Id: 0356a836e448b746fa1f83ebdfd27d039bdf6038168d4fdba6074633d1af82a4
  Sha1: c01078d0f4d7be474be6c1982f2abe6201b1a4ab
  Sha256: a396b1cb6b7e92d48f36d29002457e78b2ecc152ef93781cf8413f7bd4f1766e
  Git-Sha1: b3b0ae11c81c2e19a9cdbf4c89e878dd463081cb
  
  version https://hawser.github.com/spec/v1
  oid sha256:6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  size 20
  
This repo has just 3 file content blobs stored (small + two LFS pointers)
  $ ls "$TESTTMP"/blobstore/blobs/blob-repo0000.content.*
  $TESTTMP/blobstore/blobs/blob-repo0000.content.blake2.0356a836e448b746fa1f83ebdfd27d039bdf6038168d4fdba6074633d1af82a4
  $TESTTMP/blobstore/blobs/blob-repo0000.content.blake2.46eb1ec21f0a347eb1397b55b6b9bc3cd5a39bf5898728251c25679f987fff57
  $TESTTMP/blobstore/blobs/blob-repo0000.content.blake2.5db7cda483f4d35a023d447b8210bd317497193813e9b7ac57268f525277b509

The actual file content is not uploaded to the repo (this is the hash from pointer)
  $ mononoke_newadmin filestore -R repo fetch  --content-sha256 6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  Error: Content not found
  [1]

But it's available on the separate lfs server
  $ mononoke_newadmin filestore -R legacy_lfs fetch --content-sha256 6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  laaaaaaaaaarge file

Show that we still have all the original git objects
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"
  $ GIT_REPO_FROM_BUNDLE="${TESTTMP}/repo-git-from-bundle"
  $ mononoke_newadmin git-bundle create from-repo -R repo --output-location "$BUNDLE_PATH"
  $ git clone "$BUNDLE_PATH" "$GIT_REPO_FROM_BUNDLE"
  Cloning into '$TESTTMP/repo-git-from-bundle'...

  $ mononoke_newadmin filestore -R repo fetch --content-git-sha1 8910fc3d7dae273e6ffd1d3982af8dfc418af416
  sml fle

  $ mononoke_newadmin filestore -R repo fetch --content-git-sha1 1ab2b3357e304fef596198d92807d8d7e3580f0d
  version https://git-lfs.github.com/spec/v1
  oid sha256:6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  size 20
