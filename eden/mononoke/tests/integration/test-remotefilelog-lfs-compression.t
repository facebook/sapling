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

  $ start_and_wait_for_mononoke_server
  $ lfs_url="$(lfs_server --scuba-dataset "file://$TESTTMP/scuba.json")/repo"

Create a repo. Add a large file. Make it actually large to make sure we surface
any block size boundaries or such.

  $ hgmn_init repo
  $ cd repo
  $ yes 2>/dev/null | head -c 2MiB > large
  $ hg add large
  $ hg ci -ma
  $ hgmn push -q --to master --create
  $ cd "$TESTTMP"

Clone the repo. Take a unique cache path to go to the server, and turn off compression.

  $ cd "$TESTTMP"
  $ hgmn_clone mononoke://$(mononoke_address)/repo repo2 --noupdate --config extensions.remotenames=
  $ cd repo2
  $ setup_hg_modern_lfs "$lfs_url" 10B
  $ setconfig "remotefilelog.cachepath=$TESTTMP/cachepath2"
  $ setconfig "lfs.accept-zstd=False"

Update. Check for compression. It shouldn't be used.
(the reply to the first query is either 280 or 276 as it includes either [::1] or 127.0.0.1)

  $ hgmn up master -q
  $ sha256sum large
  76903e148255cbd5ba91d3f47fe04759afcffdf64104977fc83f688892ac0dfd  large

  $ wait_for_json_record_count "$TESTTMP/scuba.json" 2
  $ jq .int.response_content_length < "$TESTTMP/scuba.json"
  280|276 (re)
  2097152
  $ jq .int.response_bytes_sent < "$TESTTMP/scuba.json"
  280|276 (re)
  2097152
  $ jq .normal.response_content_encoding < "$TESTTMP/scuba.json"
  null
  null
  $ truncate -s 0 "$TESTTMP/scuba.json"

Clone again. This time, enable compression

  $ cd "$TESTTMP"
  $ hgmn_clone mononoke://$(mononoke_address)/repo repo3 --noupdate --config extensions.remotenames=
  $ cd repo3
  $ setup_hg_modern_lfs "$lfs_url" 10B
  $ setconfig "remotefilelog.cachepath=$TESTTMP/cachepath3"
  $ setconfig "lfs.accept-zstd=True"

Update again. This time, we should have compression.

  $ hgmn up master -q
  $ sha256sum large
  76903e148255cbd5ba91d3f47fe04759afcffdf64104977fc83f688892ac0dfd  large

  $ wait_for_json_record_count "$TESTTMP/scuba.json" 2
  $ jq .int.response_content_length < "$TESTTMP/scuba.json"
  280|276 (re)
  null
  $ jq .int.response_bytes_sent < "$TESTTMP/scuba.json"
  280|276 (re)
  202
  $ jq .normal.response_content_encoding < "$TESTTMP/scuba.json"
  null
  "zstd"
  $ truncate -s 0 "$TESTTMP/scuba.json"
