# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_pre_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  $ blobimport repo-hg/.hg repo

bonsai core data, deep, unchunked. This is the base case
  $ mononoke_walker -L sizing scrub -q -b master_bookmark -I bonsai 2>&1 | strip_glog
  Walking edge types [BookmarkToChangeset, ChangesetToBonsaiParent, ChangesetToFileContent], repo: repo
  Walking node types [Bookmark, Changeset, FileContent], repo: repo
  Seen,Loaded: 7,7, repo: repo
  * Type:Walked,Checks,Children Bookmark:1,1,2 Changeset:3,* FileContent:3,3,0, repo: repo (glob)

bonsai core data, chunked, deep, with checkpointing enabled
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset --chunk-size=2 --checkpoint-name=bonsai_deep --checkpoint-path=test_sqlite -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Repo bounds: (1, 4), repo: repo
  Starting chunk 1 with bounds (2, 4), repo: repo
  Seen,Loaded: 4,4, repo: repo
  Deferred: 1, repo: repo
  Chunk 1 inserting checkpoint (2, 4), repo: repo
  Starting chunk 2 with bounds (1, 2), repo: repo
  Seen,Loaded: 3,3, repo: repo
  Deferred: 0, repo: repo
  Chunk 2 updating checkpoint to (1, 4), repo: repo
  Completed in 2 chunks of size 2, repo: repo

inspect the checkpoint table
  $ sqlite3 "$TESTTMP/test_sqlite" "select repo_id, checkpoint_name, lower_bound, upper_bound from walker_checkpoints;"
  0|bonsai_deep|1|4

same run, but against metadata db
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset --chunk-size=2 --checkpoint-name=bonsai_deep_meta -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Repo bounds: (1, 4), repo: repo
  Starting chunk 1 with bounds (2, 4), repo: repo
  Seen,Loaded: 4,4, repo: repo
  Deferred: 1, repo: repo
  Chunk 1 inserting checkpoint (2, 4), repo: repo
  Starting chunk 2 with bounds (1, 2), repo: repo
  Seen,Loaded: 3,3, repo: repo
  Deferred: 0, repo: repo
  Chunk 2 updating checkpoint to (1, 4), repo: repo
  Completed in 2 chunks of size 2, repo: repo

test restoring from checkpoint, scrub should have no chunks to do as checkpoint loaded covers the repo bounds
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset --chunk-size=2 --checkpoint-name=bonsai_deep --checkpoint-path=test_sqlite -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Found checkpoint with bounds: (1, 4), repo: repo
  Repo bounds: (1, 4), repo: repo
  Continuing from checkpoint run 1 chunk 2 with catchup None and main None bounds, repo: repo
  Completed in 2 chunks of size 2, repo: repo

run to a new checkpoint name, with checkpoint sampling set so that last chunk not included in checkpoint
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset --chunk-size=1 --checkpoint-sample-rate=2 --checkpoint-name=bonsai_deep2 --checkpoint-path=bonsai_deep2 -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Repo bounds: (1, 4), repo: repo
  Starting chunk 1 with bounds (3, 4), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 1, repo: repo
  Starting chunk 2 with bounds (2, 3), repo: repo
  Seen,Loaded: 3,3, repo: repo
  Deferred: 1, repo: repo
  Chunk 2 inserting checkpoint (2, 4), repo: repo
  Starting chunk 3 with bounds (1, 2), repo: repo
  Seen,Loaded: 3,3, repo: repo
  Deferred: 0, repo: repo
  Completed in 3 chunks of size 1, repo: repo

run again, should have no catchup, but main bounds will continue from checkpoint
  $ sleep 1
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset --chunk-size=1 --checkpoint-name=bonsai_deep2 --checkpoint-path=bonsai_deep2 -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Found checkpoint with bounds: (2, 4), repo: repo
  Repo bounds: (1, 4), repo: repo
  Continuing from checkpoint run 1 chunk 2 with catchup None and main Some((1, 2)) bounds, repo: repo
  Starting chunk 3 with bounds (1, 2), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 0, repo: repo
  Chunk 3 updating checkpoint to (1, 4), repo: repo
  Completed in 3 chunks of size 1, repo: repo

inspect the checkpoint table, check the update time is at least one second after creation
  $ sqlite3 "$TESTTMP/bonsai_deep2" "SELECT repo_id, checkpoint_name, lower_bound, upper_bound FROM walker_checkpoints WHERE update_timestamp >= create_timestamp + 1000000000;"
  0|bonsai_deep2|1|4

additional commit
  $ cd repo-hg
  $ mkcommit D
  $ cd ..
  $ blobimport repo-hg/.hg repo

run again, should catchup with new data since checkpoint and nothing to do in main bounds
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset --chunk-size=2 --checkpoint-name=bonsai_deep --checkpoint-path=test_sqlite -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Found checkpoint with bounds: (1, 4), repo: repo
  Repo bounds: (1, 5), repo: repo
  Continuing from checkpoint run 1 chunk 2 with catchup Some((4, 5)) and main None bounds, repo: repo
  Starting chunk 3 with bounds (4, 5), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 0, repo: repo
  Completed in 3 chunks of size 2, repo: repo

setup for both a catchup due to a new commit, plus continuation from a checkpoint.  First create the partial checkpoint by setting sample rate
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset --chunk-size=1 --checkpoint-sample-rate=3 --checkpoint-name=bonsai_deep3 --checkpoint-path=test_sqlite -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Repo bounds: (1, 5), repo: repo
  Starting chunk 1 with bounds (4, 5), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 0, repo: repo
  Starting chunk 2 with bounds (3, 4), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 1, repo: repo
  Starting chunk 3 with bounds (2, 3), repo: repo
  Seen,Loaded: 3,3, repo: repo
  Deferred: 1, repo: repo
  Chunk 3 inserting checkpoint (2, 5), repo: repo
  Starting chunk 4 with bounds (1, 2), repo: repo
  Seen,Loaded: 3,3, repo: repo
  Deferred: 0, repo: repo
  Completed in 4 chunks of size 1, repo: repo

 hg setup.  First create the partial checkpoint by setting sample rate
  $ mononoke_walker -L sizing -L graph scrub -q -p BonsaiHgMapping --chunk-size=1 --checkpoint-sample-rate=3 --checkpoint-name=hg_deep --checkpoint-path=test_sqlite -I deep -i hg -i FileContent 2>&1 | strip_glog
  Repo bounds: (1, 5), repo: repo
  Starting chunk 1 with bounds (4, 5), repo: repo
  Seen,Loaded: 9,9, repo: repo
  Deferred: 0, repo: repo
  Starting chunk 2 with bounds (3, 4), repo: repo
  Seen,Loaded: 17,13, repo: repo
  Deferred: 2, repo: repo
  Starting chunk 3 with bounds (2, 3), repo: repo
  Seen,Loaded: 9,7, repo: repo
  Deferred: 1, repo: repo
  Chunk 3 inserting checkpoint (2, 5), repo: repo
  Starting chunk 4 with bounds (1, 2), repo: repo
  Seen,Loaded: 7,7, repo: repo
  Deferred: 0, repo: repo
  Completed in 4 chunks of size 1, repo: repo

OldestFirst setup for both a catchup due to a new commit, plus continuation from a checkpoint.  First create the partial checkpoint by setting sample rate
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset -d OldestFirst --chunk-size=1 --checkpoint-sample-rate=3 --checkpoint-name=bonsai_deep3_oldest --checkpoint-path=test_sqlite -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Repo bounds: (1, 5), repo: repo
  Starting chunk 1 with bounds (1, 2), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 0, repo: repo
  Starting chunk 2 with bounds (2, 3), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 0, repo: repo
  Starting chunk 3 with bounds (3, 4), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 0, repo: repo
  Chunk 3 inserting checkpoint (1, 4), repo: repo
  Starting chunk 4 with bounds (4, 5), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 0, repo: repo
  Completed in 4 chunks of size 1, repo: repo

OldestFirst hg setup
  $ mononoke_walker -L sizing -L graph scrub -q -p BonsaiHgMapping -d OldestFirst --chunk-size=1 --checkpoint-name=hg_deep_oldest --checkpoint-path=test_sqlite -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Repo bounds: (1, 5), repo: repo
  Starting chunk 1 with bounds (1, 2), repo: repo
  Seen,Loaded: 1,1, repo: repo
  Deferred: 0, repo: repo
  Chunk 1 inserting checkpoint (1, 2), repo: repo
  Starting chunk 2 with bounds (2, 3), repo: repo
  Seen,Loaded: 1,1, repo: repo
  Deferred: 0, repo: repo
  Chunk 2 updating checkpoint to (1, 3), repo: repo
  Starting chunk 3 with bounds (3, 4), repo: repo
  Seen,Loaded: 1,1, repo: repo
  Deferred: 0, repo: repo
  Chunk 3 updating checkpoint to (1, 4), repo: repo
  Starting chunk 4 with bounds (4, 5), repo: repo
  Seen,Loaded: 1,1, repo: repo
  Deferred: 0, repo: repo
  Chunk 4 updating checkpoint to (1, 5), repo: repo
  Completed in 4 chunks of size 1, repo: repo

now the additional commit
  $ cd repo-hg
  $ mkcommit E
  $ cd ..
  $ blobimport repo-hg/.hg repo

finally, bonsai should have a run with both catchup and main bounds
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset --chunk-size=1 --checkpoint-sample-rate=3 --checkpoint-name=bonsai_deep3 --checkpoint-path=test_sqlite -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Found checkpoint with bounds: (2, 5), repo: repo
  Repo bounds: (1, 6), repo: repo
  Continuing from checkpoint run 1 chunk 3 with catchup Some((5, 6)) and main Some((1, 2)) bounds, repo: repo
  Starting chunk 4 with bounds (5, 6), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 1, repo: repo
  Starting chunk 5 with bounds (1, 2), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 1, repo: repo
  Deferred edge counts by type were: ChangesetToBonsaiParent:1, repo: repo
  Completed in 5 chunks of size 1, repo: repo

hg should have a run with both catchup and main bounds, and some deferred expected at end
  $ mononoke_walker -L sizing -L graph scrub -q -p BonsaiHgMapping --chunk-size=1 --checkpoint-sample-rate=3 --checkpoint-name=hg_deep --checkpoint-path=test_sqlite -I deep -i hg 2>&1 | strip_glog
  Found checkpoint with bounds: (2, 5), repo: repo
  Repo bounds: (1, 6), repo: repo
  Continuing from checkpoint run 1 chunk 3 with catchup Some((5, 6)) and main Some((1, 2)) bounds, repo: repo
  Starting chunk 4 with bounds (5, 6), repo: repo
  Seen,Loaded: 12,9, repo: repo
  Deferred: 1, repo: repo
  Starting chunk 5 with bounds (1, 2), repo: repo
  Seen,Loaded: 8,8, repo: repo
  Deferred: 1, repo: repo
  Deferred edge counts by type were: HgChangesetToHgParent:1 HgManifestFileNodeToHgParentFileNode:1 HgManifestToHgFileNode:1, repo: repo
  Completed in 5 chunks of size 1, repo: repo

OldestFirst, has only main bounds as the start point of the repo has not changed
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset -d OldestFirst --chunk-size=1 --checkpoint-sample-rate=3 --checkpoint-name=bonsai_deep3_oldest --checkpoint-path=test_sqlite -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Found checkpoint with bounds: (1, 4), repo: repo
  Repo bounds: (1, 6), repo: repo
  Continuing from checkpoint run 1 chunk 3 with catchup None and main Some((4, 6)) bounds, repo: repo
  Starting chunk 4 with bounds (4, 5), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 0, repo: repo
  Starting chunk 5 with bounds (5, 6), repo: repo
  Seen,Loaded: 2,2, repo: repo
  Deferred: 0, repo: repo
  Completed in 5 chunks of size 1, repo: repo

OldestFirst, hg should have a run with only main bounds
  $ mononoke_walker -L sizing -L graph scrub -q -p BonsaiHgMapping -d OldestFirst --chunk-size=1 --checkpoint-name=hg_deep_oldest --checkpoint-path=test_sqlite -I deep -i hg 2>&1 | strip_glog
  Found checkpoint with bounds: (1, 5), repo: repo
  Repo bounds: (1, 6), repo: repo
  Continuing from checkpoint run 1 chunk 4 with catchup None and main Some((5, 6)) bounds, repo: repo
  Starting chunk 5 with bounds (5, 6), repo: repo
  Seen,Loaded: 12,12, repo: repo
  Deferred: 0, repo: repo
  Chunk 5 updating checkpoint to (1, 6), repo: repo
  Completed in 5 chunks of size 1, repo: repo

Check that the checkpoint low bound is not used if its too old
  $ sleep 2
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset --chunk-size=5 --state-max-age=1 --checkpoint-sample-rate=3 --checkpoint-name=bonsai_deep3 --checkpoint-path=test_sqlite -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Found checkpoint with bounds: (2, 5), repo: repo
  Repo bounds: (1, 6), repo: repo
  Checkpoint run 1 chunk 3 is too old at *s, running from repo bounds, repo: repo (glob)
  Starting chunk 1 with bounds (1, 6), repo: repo
  Seen,Loaded: 10,10, repo: repo
  Deferred: 0, repo: repo
  Completed in 1 chunks of size 5, repo: repo

OldestFirst, Check that the checkpoint high bound is not used if its too old
  $ mononoke_walker -L sizing -L graph scrub -q -p Changeset --chunk-size=5 --state-max-age=1 --checkpoint-sample-rate=3 --checkpoint-name=bonsai_deep3_oldest --checkpoint-path=test_sqlite -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Found checkpoint with bounds: (1, 4), repo: repo
  Repo bounds: (1, 6), repo: repo
  Checkpoint run 1 chunk 3 is too old at *s, running from repo bounds, repo: repo (glob)
  Starting chunk 1 with bounds (1, 6), repo: repo
  Seen,Loaded: 10,10, repo: repo
  Deferred: 0, repo: repo
  Completed in 1 chunks of size 5, repo: repo
