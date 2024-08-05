# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repository

  $ export CACHE_WARMUP_BOOKMARK="master_bookmark"
  $ export CACHE_WARMUP_MICROWAVE=1
  $ BLOB_TYPE="blob_files" quiet default_setup

Skip blobstore warmup
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:mononoke_server_skip_blobstore_warmup": true
  >   }
  > }
  > EOF

Derive data, then regenerate microwave snapshot
  $ quiet mononoke_newadmin derived-data -R repo derive --all-types --all-bookmarks
  $ quiet microwave_builder --debug blobstore

Start Mononoke again, check that the microwave snapshot was used

  $ start_and_wait_for_mononoke_server
  $ wait_for_mononoke_cache_warmup
  $ grep primed "$TESTTMP/mononoke.out"
  * primed filenodes cache with 1 entries, repo: repo (glob)
  * microwave: successfully primed cache, repo: repo (glob)

Kill Mononoke

  $ killandwait "$MONONOKE_PID"
  $ truncate -s 0 "$TESTTMP/mononoke.out"

Skip blobstore warmup
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:mononoke_server_skip_blobstore_warmup": false
  >   }
  > }
  > EOF

Test mononoke startup with justknob being false

  $ start_and_wait_for_mononoke_server
  $ wait_for_mononoke_cache_warmup
  $ grep primed "$TESTTMP/mononoke.out"
  * primed filenodes cache with 1 entries, repo: repo (glob)
  * microwave: successfully primed cache, repo: repo (glob)
