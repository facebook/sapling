# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repository

  $ export CACHE_WARMUP_BOOKMARK="master_bookmark"
  $ export CACHE_WARMUP_MICROWAVE=1
  $ BLOB_TYPE="blob_files" quiet default_setup

Check that Mononoke booted despite the lack of microwave snapshot

  $ wait_for_mononoke_cache_warmup
  $ grep microwave "$TESTTMP/mononoke.out"
  * microwave: cache warmup failed: "Snapshot is missing", repo: repo (glob)

Kill Mononoke

  $ kill "$MONONOKE_PID"
  $ truncate -s 0 "$TESTTMP/mononoke.out"

Regenerate microwave snapshot

  $ quiet microwave_builder blobstore

Start Mononoke again, check that the microwave snapshot was used

  $ mononoke
  $ wait_for_mononoke_cache_warmup
  $ grep microwave "$TESTTMP/mononoke.out"
  * microwave: successfully primed cached, repo: repo (glob)

Finally, check that we can also generate a snapshot to files

  $ mkdir "$TESTTMP/microwave"
  $ quiet microwave_builder local-path "$TESTTMP/microwave"
  $ ls "$TESTTMP/microwave"
  repo0000.microwave_snapshot_v0
