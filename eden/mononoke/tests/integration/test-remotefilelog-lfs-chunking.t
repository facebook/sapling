# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup a Mononoke repo.

  $ LFS_THRESHOLD="10" setup_common_config "blob_files"
  $ cd "$TESTTMP"

Start Mononoke & LFS.

  $ mononoke
  $ wait_for_mononoke
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_url="$(lfs_server --log "$lfs_log")/repo"

Create a repo. Add a large file. Make it actually large to make sure we surface
any block size boundaries or such.

  $ hgmn_init repo
  $ cd repo
  $ yes 2>/dev/null | head -c 2MiB > large
  $ hg add large
  $ hg ci -ma
  $ hgmn push -q --to master --create
  $ cd "$TESTTMP"

Clone the repo. Take a unique cache path to go to the server, and enable chunking.

  $ cd "$TESTTMP"
  $ hgmn_clone ssh://user@dummy/repo repo2 --noupdate --config extensions.remotenames=
  $ cd repo2
  $ setup_hg_modern_lfs "$lfs_url" 10B
  $ setconfig "remotefilelog.cachepath=$TESTTMP/cachepath2"
  $ setconfig "lfs.download-chunk-size=524288"

Update. Check for multiple requests

  $ hgmn up master -q
  $ sha256sum large
  76903e148255cbd5ba91d3f47fe04759afcffdf64104977fc83f688892ac0dfd  large

  $ cat "$lfs_log"
  IN  > POST /repo/objects/batch -
  OUT < POST /repo/objects/batch 200 OK
  IN  > GET /repo/download/ba7c3ab5dd42a490fff73f34356f5f4aa76aaf0b67d14a416bcad80a0ee8d4c9 -
  OUT < GET /repo/download/ba7c3ab5dd42a490fff73f34356f5f4aa76aaf0b67d14a416bcad80a0ee8d4c9 206 Partial Content
  IN  > GET /repo/download/ba7c3ab5dd42a490fff73f34356f5f4aa76aaf0b67d14a416bcad80a0ee8d4c9 -
  OUT < GET /repo/download/ba7c3ab5dd42a490fff73f34356f5f4aa76aaf0b67d14a416bcad80a0ee8d4c9 206 Partial Content
  IN  > GET /repo/download/ba7c3ab5dd42a490fff73f34356f5f4aa76aaf0b67d14a416bcad80a0ee8d4c9 -
  OUT < GET /repo/download/ba7c3ab5dd42a490fff73f34356f5f4aa76aaf0b67d14a416bcad80a0ee8d4c9 206 Partial Content
  IN  > GET /repo/download/ba7c3ab5dd42a490fff73f34356f5f4aa76aaf0b67d14a416bcad80a0ee8d4c9 -
  OUT < GET /repo/download/ba7c3ab5dd42a490fff73f34356f5f4aa76aaf0b67d14a416bcad80a0ee8d4c9 206 Partial Content
