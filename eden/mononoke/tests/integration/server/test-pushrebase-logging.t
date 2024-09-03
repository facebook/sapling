# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ enable pushrebase remotenames
  $ export COMMIT_SCRIBE_CATEGORY=public_commit
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 setup_common_config
  $ testtool_drawdag -R repo --derive-all --print-hg-hashes <<EOF
  > A-B-C
  > # bookmark: C main
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8
  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo
  $ hg up -q $A

Create two commits that will be rebased during pushrebase, each with different file counts and sizes
  $ echo 1 > file1.txt
  $ hg ci -Aqm commit1
  $ echo 2 > file2a.txt
  $ echo 2 > file2b.txt
  $ hg ci -Aqm commit2

Push the commits
  $ tglog
  @  bb28139b0362 'commit2'
  │
  o  a5f354cc1e5c 'commit1'
  │
  │ o  d3b399ca8757 'C'
  │ │
  │ o  80521a640a0c 'B'
  ├─╯
  o  20ca2a4749a4 'A'
  
  $ hg push -q -r . --to main

  $ tglog
  @  d6637437d715 'commit2'
  │
  o  0007004744d1 'commit1'
  │
  o  d3b399ca8757 'C'
  │
  o  80521a640a0c 'B'
  │
  o  20ca2a4749a4 'A'
  

Check the logs, ensure that each commit (based on its generation number) has the right changed files count
and size.
  $ jq < $TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY -c '[.generation, .changed_files_count, .changed_files_size]' | sort
  [2,1,2]
  [3,2,4]
  [4,1,2]
  [5,2,4]
