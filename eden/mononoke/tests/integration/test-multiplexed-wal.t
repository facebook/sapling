# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=3 WAL=1 REPOTYPE=blob_files setup_common_config
  $ cd $TESTTMP
  $ configure modernclient

setup repo
  $ testtool_drawdag -R repo << EOF
  > B
  > |
  > A
  > # bookmark: B main
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
start mononoke
  $ start_and_wait_for_mononoke_server


clone
  $ hgedenapi clone -q  "mononoke://$(mononoke_address)/repo" client1
  $ cd client1

Push
  $ echo 1 > 1 && quiet hgedenapi commit -A -m 1
  $ echo "$(read_blobstore_wal_queue_size)"
  20
  $ hgedenapi push -r . --to main
  pushing rev 523cda1e6192 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
  searching for changes
  updating bookmark main

Count number of entries the blobstore sync queue
  $ echo "$(read_blobstore_wal_queue_size)"
  36
  $ cat "$TESTTMP/blobstore_trace_scuba.json" | jq 'select(.normal.operation=="put" and (.normal.key | contains(".changeset."))) | 1' | wc -l
  6
