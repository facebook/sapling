# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false
  $ setconfig push.edenapi=true
  $ BLOB_TYPE="blob_files" default_setup_drawdag --scuba-dataset "file://$TESTTMP/log.json"
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Pushrebase commit 1
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg push -r . --to master_bookmark
  pushing rev 26f143b427a3 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (20ca2a4749a4, 26f143b427a3] (1 commit) to remote bookmark master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to c39a1f67cdbc
  $ mononoke_newadmin derived-data -R repo derive -T filenodes --all-bookmarks

  $ log -r "all()"
  @  1 [public;rev=4;c39a1f67cdbc] remote/master_bookmark
  │
  o  C [public;rev=2;d3b399ca8757]
  │
  o  B [public;rev=1;80521a640a0c]
  │
  o  A [public;rev=0;20ca2a4749a4]
  $

Pushrebased commit 1 over commits B and C (thus the distance should be 2).
  $ jq < "$TESTTMP/log.json" '.int.pushrebase_distance | numbers' | tail -n 1
  2

Check that the filenode for 1 does not point to the draft commit in a new clone
  $ cd ..
  $ hg clone -q mono:repo repo3 --noupdate
  $ cd repo3
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

  $ hg pull -r master_bookmark
  pulling from mono:repo
  $ hg up master_bookmark
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugsh -c 'ui.write("%s\n" % s.node.hex(repo["."].filectx("1").getnodeinfo()[2]))'
  c39a1f67cdbc38a5701bef538d354d47b7c9f2cb
  $ cd ../repo

Push rebase fails with conflict in the bottom of the stack
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ hg push -r . --to master_bookmark
  pushing rev 8b01ec816b8a to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (20ca2a4749a4, 8b01ec816b8a] (2 commits) to remote bookmark master_bookmark
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: NonRootMPath("1"), right: NonRootMPath("1") }]
  [255]
  $ hg hide -r ".^ + ." -q


Push rebase fails with conflict in the top of the stack
  $ hg up -q "min(all())"
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg push -r . --to master_bookmark
  pushing rev be73549636d1 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  pushrebasing stack (20ca2a4749a4, be73549636d1] (2 commits) to remote bookmark master_bookmark
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: NonRootMPath("1"), right: NonRootMPath("1") }]
  [255]
  $ hg hide -r ".^ + ." -q


Push stack
  $ hg up -q "min(all())"
  $ echo 3 > 3 && hg add 3 && hg ci -m 3
  $ echo 4 > 4 && hg add 4 && hg ci -m 4
  $ hg push -r . --to master_bookmark
  pushing rev 1096b7ced56c to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  pushrebasing stack (20ca2a4749a4, 1096b7ced56c] (2 commits) to remote bookmark master_bookmark
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 6c33ca0cbd4d
  $ hg up -q master_bookmark
  $ log -r "all()"
  @  4 [public;rev=11;6c33ca0cbd4d] remote/master_bookmark
  │
  o  3 [public;rev=10;663c2d9c201e]
  │
  o  1 [public;rev=4;c39a1f67cdbc]
  │
  o  C [public;rev=2;d3b399ca8757]
  │
  o  B [public;rev=1;80521a640a0c]
  │
  o  A [public;rev=0;20ca2a4749a4]
  $

Pushrebased commits {3, 4} over commits {B, C, 1} (thus the distance should be 3).
  $ jq < "$TESTTMP/log.json" '.int.pushrebase_distance | numbers' | tail -n 1
  3

Push fast-forward
  $ hg up master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ OLD_HASH="$(hg whereami)"
  $ echo 5 > 5 && hg add 5 && hg ci -m 5
  $ hg push -r . --to master_bookmark
  pushing rev b9f277aaa224 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (6c33ca0cbd4d, b9f277aaa224] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to b9f277aaa224
  $ log -r "all()"
  @  5 [public;rev=12;b9f277aaa224] remote/master_bookmark
  │
  o  4 [public;rev=11;6c33ca0cbd4d]
  │
  o  3 [public;rev=10;663c2d9c201e]
  │
  o  1 [public;rev=4;c39a1f67cdbc]
  │
  o  C [public;rev=2;d3b399ca8757]
  │
  o  B [public;rev=1;80521a640a0c]
  │
  o  A [public;rev=0;20ca2a4749a4]
  $
  $ jq < "$TESTTMP/log.json" '.int.pushrebase_distance | numbers' | tail -n 1
  0


Push with no new commits
  $ hg push -r . --to master_bookmark
  pushing rev b9f277aaa224 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  moving remote bookmark master_bookmark from b9f277aaa224 to b9f277aaa224
  $ log -r "."
  @  5 [public;rev=12;b9f277aaa224] remote/master_bookmark
  │
  ~

Push a merge commit with both parents not ancestors of destination bookmark
  $ hg up -q 1
  $ echo 6 > 6 && hg add 6 && hg ci -m 6
  $ hg up -q 1
  $ echo 7 > 7 && hg add 7 && hg ci -m 7
  $ hg merge -q -r 13 && hg ci -m "merge 6 and 7"
  $ log -r "all()"
  @    merge 6 and 7 [draft;rev=15;3216c28e1752]
  ├─╮
  │ o  7 [draft;rev=14;963a2e3bcf35]
  │ │
  o │  6 [draft;rev=13;22cfc5c8c7f6]
  ├─╯
  │ o  5 [public;rev=12;b9f277aaa224] remote/master_bookmark
  │ │
  │ o  4 [public;rev=11;6c33ca0cbd4d]
  │ │
  │ o  3 [public;rev=10;663c2d9c201e]
  │ │
  │ o  1 [public;rev=4;c39a1f67cdbc]
  │ │
  │ o  C [public;rev=2;d3b399ca8757]
  ├─╯
  o  B [public;rev=1;80521a640a0c]
  │
  o  A [public;rev=0;20ca2a4749a4]
  $

  $ hg push -r . --to master_bookmark
  fallback reason: merge commit is not supported by EdenApi push yet
  pushing rev 3216c28e1752 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  $ hg up master_bookmark -q && hg hide -r "13+14+15" -q
  $ log -r "all()"
  @    merge 6 and 7 [public;rev=18;98f9e08333e0] remote/master_bookmark
  ├─╮
  │ o  7 [public;rev=17;5c369a3ce002]
  │ │
  o │  6 [public;rev=16;18e02fd69ab4]
  ├─╯
  o  5 [public;rev=12;b9f277aaa224]
  │
  o  4 [public;rev=11;6c33ca0cbd4d]
  │
  o  3 [public;rev=10;663c2d9c201e]
  │
  o  1 [public;rev=4;c39a1f67cdbc]
  │
  o  C [public;rev=2;d3b399ca8757]
  │
  o  B [public;rev=1;80521a640a0c]
  │
  o  A [public;rev=0;20ca2a4749a4]
  $
  $ jq < "$TESTTMP/log.json" '.int.pushrebase_distance | numbers' | tail -n 1
  5


Previously commits below were testing pushrebasing over merge.
Keep them in place to not change the output for all the tests below
  $ hg up 11 -q
  $ echo 8 > 8 && hg add 8 && hg ci -m 8
  $ hg up master_bookmark -q

Push-rebase of a commit with p2 being the ancestor of the destination bookmark
- Do some preparatory work
  $ echo 9 > 9 && hg add 9 && hg ci -m 9
  $ echo 10 > 10 && hg add 10 && hg ci -m 10
  $ echo 11 > 11 && hg add 11 && hg ci -m 11
  $ hg push -r . --to master_bookmark -q
  $ hg up .^^ && echo 12 > 12 && hg add 12 && hg ci -m 12
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -r master_bookmark -T '{node}\n'
  45d2d9680dec59f71e8cbfd4b5c0fa9ec4c1215c

  $ hg merge -qr 21 && hg ci -qm "merge 10 and 12"
  $ hg phase -r $(hg log -r . -T "{p1node}")
  67c6d19149569d9a7ff24f480a1e0391038108de: draft
  $ hg phase -r $(hg log -r . -T "{p2node}")
  715b5a51388191187b4157554963250980b7ce45: public
  $ hg log -r master_bookmark -T '{node}\n'
  45d2d9680dec59f71e8cbfd4b5c0fa9ec4c1215c

- Actually test the push
  $ hg push -r . --to master_bookmark
  fallback reason: merge commit is not supported by EdenApi push yet
  pushing rev f1e6d3dc240b to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  $ hg hide -r . -q && hg up master_bookmark -q
  $ hg log -r master_bookmark -T '{node}\n'
  1065de83df59887b118346ca57704ee2326b4cb9
Test creating a bookmark on a public commit
  $ hg push --rev 25 --to master_bookmark_2 --create
  pushing rev 1065de83df59 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark_2
  creating remote bookmark master_bookmark_2
  $ log -r "20::"
  @    merge 10 and 12 [public;rev=25;1065de83df59] remote/master_bookmark remote/master_bookmark_2
  ├─╮
  │ o  12 [public;rev=23;67c6d1914956]
  │ │
  o │  11 [public;rev=22;45d2d9680dec]
  │ │
  o │  10 [public;rev=21;715b5a513881]
  ├─╯
  o  9 [public;rev=20;99a124548fb0]
  │
  ~

Test a non-forward push
  $ hg up 22 -q
  $ log -r "20::"
  o    merge 10 and 12 [public;rev=25;1065de83df59] remote/master_bookmark remote/master_bookmark_2
  ├─╮
  │ o  12 [public;rev=23;67c6d1914956]
  │ │
  @ │  11 [public;rev=22;45d2d9680dec]
  │ │
  o │  10 [public;rev=21;715b5a513881]
  ├─╯
  o  9 [public;rev=20;99a124548fb0]
  │
  ~
  $ hg push --force -r . --to master_bookmark_2 --non-forward-move --pushvar NON_FAST_FORWARD=true
  pushing rev 45d2d9680dec to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark_2
  moving remote bookmark master_bookmark_2 from 1065de83df59 to 45d2d9680dec
  $ log -r "20::"
  o    merge 10 and 12 [public;rev=25;1065de83df59] remote/master_bookmark
  ├─╮
  │ o  12 [public;rev=23;67c6d1914956]
  │ │
  @ │  11 [public;rev=22;45d2d9680dec] remote/master_bookmark_2
  │ │
  o │  10 [public;rev=21;715b5a513881]
  ├─╯
  o  9 [public;rev=20;99a124548fb0]
  │
  ~

Test deleting a bookmark
  $ hg push --delete master_bookmark_2
  deleting remote bookmark master_bookmark_2
  $ log -r "20::"
  o    merge 10 and 12 [public;rev=25;1065de83df59] remote/master_bookmark
  ├─╮
  │ o  12 [public;rev=23;67c6d1914956]
  │ │
  @ │  11 [public;rev=22;45d2d9680dec]
  │ │
  o │  10 [public;rev=21;715b5a513881]
  ├─╯
  o  9 [public;rev=20;99a124548fb0]
  │
  ~

Test creating a bookmark and new head
  $ echo draft > draft && hg add draft && hg ci -m draft
  $ hg push -r . --to newbook --create
  pushing rev db727ac656f2 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark newbook
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  creating remote bookmark newbook

Test non-fast-forward force pushrebase
  $ hg up -qr 20
  $ echo Aeneas > was_a_lively_fellow && hg ci -qAm 26
  $ log -r "20::"
  @  26 [draft;rev=27;10b6d4d89240]
  │
  │ o  draft [public;rev=26;db727ac656f2] remote/newbook
  │ │
  │ │ o  merge 10 and 12 [public;rev=25;1065de83df59] remote/master_bookmark
  │ ╭─┤
  │ │ o  12 [public;rev=23;67c6d1914956]
  ├───╯
  │ o  11 [public;rev=22;45d2d9680dec]
  │ │
  │ o  10 [public;rev=21;715b5a513881]
  ├─╯
  o  9 [public;rev=20;99a124548fb0]
  │
  ~
-- we don't need to pass --pushvar NON_FAST_FORWARD if we're doing a force pushrebase
  $ hg push -r . -f --to newbook
  pushing rev 10b6d4d89240 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark newbook
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  moving remote bookmark newbook from db727ac656f2 to 10b6d4d89240
  $ mononoke_newadmin derived-data -R repo derive -T filenodes --all-bookmarks
-- "20 draft newbook" gets moved to 26 and 20 gets hidden.
  $ log -r "20::"
  @  26 [public;rev=27;10b6d4d89240] remote/newbook
  │
  │ o    merge 10 and 12 [public;rev=25;1065de83df59] remote/master_bookmark
  │ ├─╮
  │ │ o  12 [public;rev=23;67c6d1914956]
  ├───╯
  │ o  11 [public;rev=22;45d2d9680dec]
  │ │
  │ o  10 [public;rev=21;715b5a513881]
  ├─╯
  o  9 [public;rev=20;99a124548fb0]
  │
  ~

-- Check that pulling a force pushrebase has good linknodes.
  $ cd ../repo3
  $ hg pull -B newbook
  pulling from mono:repo
  searching for changes
  fetching revlog data for 8 commits
  $ hg up newbook
  7 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugsh -c 'ui.write("%s\n" % s.node.hex(repo["."].filectx("was_a_lively_fellow").getnodeinfo()[2]))'
  10b6d4d892408c3005dcd233c3a8cc470246aba5
  $ cd ../repo

Check that a force pushrebase with mutation markers.
  $ echo SPARTACUS > sum_ego && hg ci -qAm 27
  $ echo SPARTACUS! > sum_ego && hg amend --config mutation.enabled=true --config mutation.record=true
  $ hg push -r . -f --to newbook --config push.check-mutation=true
  pushing rev * to destination https://localhost:*/edenapi/ bookmark newbook (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  abort: forced push blocked because commit * contains mutation metadata (glob)
  (use 'hg amend --config mutation.record=false' to remove the metadata)
  [255]

Check that we can replace a file with a directory
  $ cd ../$REPONAME
  $ hg up remote/newbook -q
  $ hg rm A -q
  $ mkdir A
  $ echo hello > A/hello
  $ hg add A/hello -q
  $ hg ci -qm "replace a file with a dir"
  $ hg push --to newbook
  pushing rev 8294354c0a10 to destination https://localhost:*/edenapi/ bookmark newbook (glob)
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (10b6d4d89240, 8294354c0a10] (1 commit) to remote bookmark newbook
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark newbook to 8294354c0a10

  $ ls A
  hello
  $ log -r "30"
  @  replace a file with a dir [public;rev=30;8294354c0a10] remote/newbook
  │
  ~
