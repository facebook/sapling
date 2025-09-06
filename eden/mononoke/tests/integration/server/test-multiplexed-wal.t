# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=3 REPOTYPE=blob_files setup_common_config
  $ cd $TESTTMP
  $ configure modernclient

setup repo
  $ testtool_drawdag -R repo << EOF
  > B
  > |
  > A
  > # bookmark: B master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
start mononoke
  $ start_and_wait_for_mononoke_server


clone
  $ hg clone -q  mono:repo client1
  $ cd client1

Push
  $ echo 1 > 1 && quiet hg commit -A -m 1
We need to run the blobstore healer to clear the queue (i.e., flush the pending items into all replicas),
otherwise, the test is going to be flaky becasue of the eventual consistency model employed by our system.
  $ mononoke_blobstore_healer -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 > /dev/null
  $ echo "$(read_blobstore_wal_queue_size)"
  0
  $ hg push -r . --to master_bookmark
  pushing rev 523cda1e6192 to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

Count number of entries the blobstore sync queue
  $ echo "$(read_blobstore_wal_queue_size)"
  0
  $ cat "$TESTTMP/blobstore_trace_scuba.json" | jq 'select(.normal.operation=="put" and (.normal.key | contains(".changeset."))) | 1' | wc -l
  6

Fetch blob with monad
  $ mononoke_admin fetch -R repo -B master_bookmark 
  BonsaiChangesetId: b7f8e4ac0f4cd74eb44dcace531fc23608e428f0ae71213a6734ec4ae54641fb
  Author: test
  Message: 1
  FileChanges:
  	 ADDED/MODIFIED: 1 b354ba2566c63fedc28780add52b066d6428ef596f57fa2e50c094d0fcf41c00
  

Disable all blobstores
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:disable_blobstore_reads": true
  >   }
  > }
  > EOF

Try again and fail because reads for all blobstores will be disabled
  $ mononoke_admin fetch -R repo -B master_bookmark 
  Error: Failed to load changeset b7f8e4ac0f4cd74eb44dcace531fc23608e428f0ae71213a6734ec4ae54641fb
  
  Caused by:
      All blobstores failed: {}
  [1]
  $ force_update_configerator
  $ killandwait $MONONOKE_PID
  $ mononoke --scuba-log-file "$TESTTMP/log.json"
  $ wait_for_mononoke

  $ hg clone -q mono:repo repo-clone
  abort: cannot resolve [523cda1e6192b3b1e4208793ee19bd67125c6a93] remotely
  [255]
  $ jq . $TESTTMP/log.json | rg "disabled_reads_blobstore_ids" -A 5 -m 2
      "disabled_reads_blobstore_ids": [
        "1",
        "2",
        "3"
      ],
      "use_maybe_stale_freshness_for_bookmarks": [
  --
      "disabled_reads_blobstore_ids": [
        "1",
        "2",
        "3"
      ],
      "use_maybe_stale_freshness_for_bookmarks": [


Enable all blobstores again
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:disable_blobstore_reads": false
  >   }
  > }
  > EOF

Everything should work again
  $ mononoke_admin fetch -R repo -B master_bookmark 
  BonsaiChangesetId: b7f8e4ac0f4cd74eb44dcace531fc23608e428f0ae71213a6734ec4ae54641fb
  Author: test
  Message: 1
  FileChanges:
  	 ADDED/MODIFIED: 1 b354ba2566c63fedc28780add52b066d6428ef596f57fa2e50c094d0fcf41c00
  
