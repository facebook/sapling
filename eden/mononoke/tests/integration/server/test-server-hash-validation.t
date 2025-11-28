# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_files"
  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo --no-default-files <<'EOF'
  > C
  > |
  > B
  > |
  > A
  > # modify: A "file1" "content1\n"
  > # modify: B "file1" "content1\n"
  > # modify: B "file2" "content2\n"
  > # modify: C "file1" "content1\n"
  > # modify: C "file2" "content2\n"
  > # modify: C "file3" "content3\n"
  > # message: A "A"
  > # message: B "B"
  > # message: C "C"
  > # bookmark: C master_bookmark
  > EOF
  A=* (glob)
  B=* (glob)
  C=* (glob)

  $ cd $TESTTMP

start mononoke
  $ start_and_wait_for_mononoke_server

setup client repo2
  $ hg clone -q mono:repo repo-client --noupdate
  $ cd repo-client

Stop mononoke before corrupting blobs
  $ killandwait $MONONOKE_PID

Corrupt blobs by replacing one content blob with another
  $ cd $TESTTMP/blobstore/blobs
  $ FIRST_BLOB=$(ls blob-repo0000.content.blake2.* | head -1 | xargs basename)
  $ SECOND_BLOB=$(ls blob-repo0000.content.blake2.* | head -2 | tail -1 | xargs basename)
  $ cp "$FIRST_BLOB" "$SECOND_BLOB"

Restart mononoke to pick up corrupted blobs
  $ rm -rf "$TESTTMP/mononoke_logs"
  $ start_and_wait_for_mononoke_server

Prefetch should fail with corruption error
  $ cd $TESTTMP/repo-client
  $ hg pull --config ui.disable-stream-clone=true
  pulling from mono:repo

  $ LOG=revisionstore=debug hg prefetch -r ":" 2>&1 | grep "Invalid hash"
  * Errors = 1, Error = Some("0eb86721b74ed44cf176ee48b5e95f0192dc2824 : Invalid hash: 0eb86721b74ed44cf176ee48b5e95f0192dc2824 (expected) != 07c0d950fdeb8c7d82ae7f15b6d1cb7f330da8a7 (computed)") (glob)
