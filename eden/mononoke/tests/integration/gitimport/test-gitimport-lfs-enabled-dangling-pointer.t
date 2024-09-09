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
  $ GIT_LFS_INTERPRET_POINTERS=1 test_repos_for_git_lfs_import
  $ cd "$GIT_REPO_CLIENT"
  $ git lfs uninstall --local --skip-repo
  $ git config lfs.allowincompletepush true
  $ quiet git reset --hard
  $ cat >> large_file_dangling_pointer <<EOF
  > version https://git-lfs.github.com/spec/v1
  > oid sha256:baaaaaaddddd0000000000000000000000000000000000000000000000000000
  > size 1234
  > EOF
  $ git add large_file_dangling_pointer
  $ git commit -aqm "add dangling pointer"
  $ quiet git push -q origin main
  Uploading LFS objects:   0% (0/1), 0 B | 0 B/s, done. (?)

But it's available on the separate lfs server
  $ mononoke_newadmin filestore -R legacy_lfs fetch --content-sha256 6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  laaaaaaaaaarge file

Git Import without extra option will fail
  $ with_stripped_logs gitimport "$GIT_REPO_SERVER" --lfs-import-max-attempts 1 --generate-bookmarks --concurrency 100 --lfs-server "$LEGACY_LFS_URL/download_sha256" full-repo | grep -e error -e Execution
  Execution error: gitimport failed
  Error: Execution failed

Git Import will skip dangling pointer
  $ quiet_grep Uploading -- with_stripped_logs gitimport "$GIT_REPO_SERVER"  --lfs-import-max-attempts 1 --allow-dangling-lfs-pointers --generate-bookmarks --concurrency 100 --lfs-server "$LEGACY_LFS_URL/download_sha256" full-repo | sort
  Uploading LFS large_file sha256:6c54a4de size:20
  Uploading LFS large_file_dangling_pointer sha256:baaaaaad size:1234
  Uploading LFS large_file_non_canonical_pointer sha256:6c54a4de size:20
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
  Size: 20
  Content-Id: 48ef00ac63821b09154b55f1b380d253f936afb076a873e1bcc1d137c8b5bab2
  Sha1: b9b10245bc406126987c342d363d89fb5b228fc7
  Sha256: 6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  Git-Sha1: 11aadb9485d337b846e0d64ff8f575b5b36ed0a8
  
  laaaaaaaaaarge file
  
  $ mononoke_newadmin fetch -R repo -B heads/main --path large_file_non_canonical_pointer
  File-Type: regular
  Size: 20
  Content-Id: 48ef00ac63821b09154b55f1b380d253f936afb076a873e1bcc1d137c8b5bab2
  Sha1: b9b10245bc406126987c342d363d89fb5b228fc7
  Sha256: 6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  Git-Sha1: 11aadb9485d337b846e0d64ff8f575b5b36ed0a8
  
  laaaaaaaaaarge file
  
This repo has 3 file content blobs stored (small + two LFS pointers + one large content)
  $ ls "$TESTTMP"/blobstore/blobs/blob-repo0000.content.*
  $TESTTMP/blobstore/blobs/blob-repo0000.content.blake2.0356a836e448b746fa1f83ebdfd27d039bdf6038168d4fdba6074633d1af82a4
  $TESTTMP/blobstore/blobs/blob-repo0000.content.blake2.0809a96fbfd58f5d4f4074754d701540354e3a6946326bf284665a824eb054a4
  $TESTTMP/blobstore/blobs/blob-repo0000.content.blake2.46eb1ec21f0a347eb1397b55b6b9bc3cd5a39bf5898728251c25679f987fff57
  $TESTTMP/blobstore/blobs/blob-repo0000.content.blake2.48ef00ac63821b09154b55f1b380d253f936afb076a873e1bcc1d137c8b5bab2
  $TESTTMP/blobstore/blobs/blob-repo0000.content.blake2.5db7cda483f4d35a023d447b8210bd317497193813e9b7ac57268f525277b509

The actual file content is uploaded to the repo (this is the hash from pointer)
  $ mononoke_newadmin filestore -R repo fetch  --content-sha256 6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  laaaaaaaaaarge file

Show that we still have all the original git objects
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"
  $ GIT_REPO_FROM_BUNDLE="${TESTTMP}/repo-git-from-bundle"
  $ mononoke_newadmin git-bundle create from-repo -R repo --output-location "$BUNDLE_PATH"
  $ git clone "$BUNDLE_PATH" "$GIT_REPO_FROM_BUNDLE"
  Cloning into '$TESTTMP/repo-git-from-bundle'...
  $ cd "$GIT_REPO_CLIENT"
  $ git cat-file -p HEAD
  tree a29ca711342d3cf022d5da7b59c86009de3dc94c
  parent c13a0ad234813977286c5827533de22af7f04fc5
  author mononoke <mononoke@mononoke> 946684800 +0000
  committer mononoke <mononoke@mononoke> 946684800 +0000
  
  add dangling pointer

  $ git cat-file -p afae45be853e0e99e21ef1b1a0beba60e41d9753
  100644 blob 1ab2b3357e304fef596198d92807d8d7e3580f0d	large_file
  100644 blob 8910fc3d7dae273e6ffd1d3982af8dfc418af416	small_file

  $ mononoke_newadmin filestore -R repo fetch --content-git-sha1 8910fc3d7dae273e6ffd1d3982af8dfc418af416
  sml fle
  $ mononoke_newadmin filestore -R repo fetch --content-git-sha1 1ab2b3357e304fef596198d92807d8d7e3580f0d
  version https://git-lfs.github.com/spec/v1
  oid sha256:6c54a4de10537e482e9f91281fb85ab614e0e0f62307047f9b9f3ccea2de8204
  size 20

Inspect bonsai change
  $ mononoke_newadmin fetch -R repo -B heads/main
  BonsaiChangesetId: 9fdf792450a6f263f7a3417827e261709fcc14a521e24fb60dde0346300eda2a
  Author: mononoke <mononoke@mononoke>
  Message: add dangling pointer
  
  FileChanges:
  	 ADDED/MODIFIED: large_file_dangling_pointer 0809a96fbfd58f5d4f4074754d701540354e3a6946326bf284665a824eb054a4
  

  $ mononoke_newadmin git-objects -R repo fetch --id afae45be853e0e99e21ef1b1a0beba60e41d9753
  The object is a Git Tree
  
  Tree {
      entries: [
          Entry {
              mode: EntryMode(
                  33188,
              ),
              filename: "large_file",
              oid: Sha1(1ab2b3357e304fef596198d92807d8d7e3580f0d),
          },
          Entry {
              mode: EntryMode(
                  33188,
              ),
              filename: "small_file",
              oid: Sha1(8910fc3d7dae273e6ffd1d3982af8dfc418af416),
          },
      ],
  }
