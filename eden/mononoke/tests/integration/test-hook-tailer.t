# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" quiet default_setup

Run the hook tailer

  $ hook_tailer --bookmark master_bookmark 2>&1 | strip_glog
  Hook tailer is starting
  *Reloading redacted config from configerator* (glob)
  ==== Hooks results ====
  Starting hooks for c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd (0 already started)
  ==== Hooks stats ====
  Completion time: *us (glob)
  Poll time: *us (glob)
  Changesets accepted: 3
  Changesets rejected: 0

Test the CSV output
  $ quiet hook_tailer --bookmark master_bookmark --stats-file "$TESTTMP/stats.csv"
  $ head -n 1 "$TESTTMP/stats.csv"
  Changeset ID,File Count,Outcomes,Completion Time us,Poll Time us
  $ tail -n +2 "$TESTTMP/stats.csv" | sort
  459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66,1,0,*,* (glob)
  9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec,1,0,*,* (glob)
  c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd,1,0,*,* (glob)

Test various combinations of exclusions

  $ hook_tailer --bookmark master_bookmark --limit 2 2>&1 | strip_glog
  Hook tailer is starting
  *Reloading redacted config from configerator* (glob)
  ==== Hooks results ====
  Starting hooks for c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd (0 already started)
  ==== Hooks stats ====
  Completion time: *us (glob)
  Poll time: *us (glob)
  Changesets accepted: 2
  Changesets rejected: 0

  $ hook_tailer --bookmark master_bookmark --exclude master_bookmark 2>&1 | strip_glog
  Hook tailer is starting
  *Reloading redacted config from configerator* (glob)
  changeset resolved as: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd))
  ==== Hooks results ====
  Starting hooks for 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66 (0 already started)
  ==== Hooks stats ====
  Completion time: *us (glob)
  Poll time: *us (glob)
  Changesets accepted: 2
  Changesets rejected: 0

  $ echo "master_bookmark" > "$TESTTMP/excluded"
  $ hook_tailer --bookmark master_bookmark --exclude-file "$TESTTMP/excluded" 2>&1 | strip_glog
  Hook tailer is starting
  *Reloading redacted config from configerator* (glob)
  changeset resolved as: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd))
  ==== Hooks results ====
  Starting hooks for 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66 (0 already started)
  ==== Hooks stats ====
  Completion time: *us (glob)
  Poll time: *us (glob)
  Changesets accepted: 2
  Changesets rejected: 0

Test excluding multiple commits

  $ hook_tailer --bookmark master_bookmark --exclude 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66 --exclude 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec 2>&1 | strip_glog
  Hook tailer is starting
  *Reloading redacted config from configerator* (glob)
  changeset resolved as: ChangesetId(Blake2(*)) (glob)
  changeset resolved as: ChangesetId(Blake2(*)) (glob)
  ==== Hooks results ====
  Starting hooks for c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd (0 already started)
  ==== Hooks stats ====
  Completion time: *us (glob)
  Poll time: *us (glob)
  Changesets accepted: 1
  Changesets rejected: 0

Test explicit commits

  $ hook_tailer --bookmark master_bookmark --changeset "master_bookmark" 2>&1 | strip_glog
  Hook tailer is starting
  *Reloading redacted config from configerator* (glob)
  changeset resolved as: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd))
  ==== Hooks results ====
  Starting hooks for c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd (0 already started)
  ==== Hooks stats ====
  Completion time: *us (glob)
  Poll time: *us (glob)
  Changesets accepted: 1
  Changesets rejected: 0

  $ hook_tailer --bookmark master_bookmark --changeset 459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66 --changeset 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec 2>&1 | strip_glog
  Hook tailer is starting
  *Reloading redacted config from configerator* (glob)
  changeset resolved as: ChangesetId(Blake2(*)) (glob)
  changeset resolved as: ChangesetId(Blake2(*)) (glob)
  ==== Hooks results ====
  Starting hooks for * (0 already started) (glob)
  ==== Hooks stats ====
  Completion time: *us (glob)
  Poll time: *us (glob)
  Changesets accepted: 2
  Changesets rejected: 0

  $ echo "459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66" >> "$TESTTMP/included"
  $ echo "9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec" >> "$TESTTMP/included"
  $ hook_tailer --bookmark master_bookmark --changeset-file "$TESTTMP/included" 2>&1 | strip_glog
  Hook tailer is starting
  *Reloading redacted config from configerator* (glob)
  changeset resolved as: ChangesetId(Blake2(*)) (glob)
  changeset resolved as: ChangesetId(Blake2(*)) (glob)
  ==== Hooks results ====
  Starting hooks for * (0 already started) (glob)
  ==== Hooks stats ====
  Completion time: *us (glob)
  Poll time: *us (glob)
  Changesets accepted: 2
  Changesets rejected: 0
