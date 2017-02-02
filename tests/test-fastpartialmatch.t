
  $ mkcommit() {
  >  echo "$1" > "$1"
  >  hg add "$1"
  >  hg ci -m "$1"
  > }

  $ extpath=`dirname $TESTDIR`
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastpartialmatch=$extpath/hgext3rd/fastpartialmatch.py
  > strip=
  > histedit=
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF

  $ hg init repo
  $ cd repo
  $ hg debugrebuildpartialindex
  $ mkcommit "first"
  $ hg debugcheckpartialindex
  $ hg log -r . -T '{node}\n'
  b75a450e74d5a7708da8c3144fbeb4ac88694044

Check permissions
  $ ls -al .hg/store/partialindex/
  total 12
  drwxr-xr-x. 2 stash users 4096 .* \. (re)
  drwxr-xr-x. 4 stash users 4096 .* \.\. (re)
  -rw-r--r--. 1 stash users   48 .* b7 (re)

Check debug commands
  $ hg debugrebuildpartialindex
  $ hg debugcheckpartialindex
  $ hg debugprintpartialindexfile
  abort: please specify a filename
  [255]
  $ hg debugprintpartialindexfile unknownfile
  file unknownfile does not exist
  $ hg debugprintpartialindexfile b7
  b75a450e74d5a7708da8c3144fbeb4ac88694044 0

Check that debugcheckpartialindex fails on corrupted indexes
  $ hg debugcheckpartialindex
  $ rm .hg/store/partialindex/b7
  $ hg debugcheckpartialindex
  b75a450e74d5a7708da8c3144fbeb4ac88694044 node not found in partialindex
  [1]
  $ printf 'garbage' > .hg/store/partialindex/b7
  $ hg debugcheckpartialindex
  b7 file is corrupted: corrupted header: run `hg debugrebuildpartialindex` to fix the issue
  b75a450e74d5a7708da8c3144fbeb4ac88694044 node not found in partialindex
  [1]
  $ hg log -r b75a
  failed to read partial index partialindex/b7 : corrupted header: run `hg debugrebuildpartialindex` to fix the issue
  failed to read partial index partialindex/b7 : corrupted header: run `hg debugrebuildpartialindex` to fix the issue
  failed to read partial index partialindex/b7 : corrupted header: run `hg debugrebuildpartialindex` to fix the issue
  failed to read partial index partialindex/b7 : corrupted header: run `hg debugrebuildpartialindex` to fix the issue
  changeset:   0:b75a450e74d5
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     first
  
  $ mkcommit committostrip
  $ hg log -r . -T '{node}'
  1138fa1e0b22411fc96c825c2603c5c3d056a206 (no-eol)
  $ hg debugrebuildpartialindex
  $ mv .hg/store/partialindex .hg/store/tmppartialindex
  $ hg strip .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/1138fa1e0b22-27b827b8-backup.hg (glob)
  $ mv .hg/store/tmppartialindex .hg/store/partialindex
  $ hg debugcheckpartialindex
  abort: 00changelog.i@1138fa1e0b22: no node!
  [255]

  $ hg debugrebuildpartialindex
  $ hg debugcheckpartialindex

Resolve 0 revision. Make sure index is not used
  $ hg log -r 0 --debug | egrep 'changeset|using partial index cache'
  changeset:   0:b75a450e74d5a7708da8c3144fbeb4ac88694044

Resolve by commit hash prefix. Make sure index is used
  $ hg log -r b75a --debug | egrep 'changeset|using partial index cache'
  using partial index cache 0
  using partial index cache 0
  changeset:   0:b75a450e74d5a7708da8c3144fbeb4ac88694044

Try to resolve unknown hash
  $ hg log -r ololo
  abort: unknown revision 'ololo'!
  [255]

Test raiseifinconsistent option
  $ rm .hg/store/partialindex/b7
  $ hg log -r b75a
  changeset:   0:b75a450e74d5
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     first
  
  $ hg log --config fastpartialmatch.raiseifinconsistent=True -r b75a 2>&1 | grep ValueError
  ValueError: inconsistent partial match index while resolving b75a

Test that new partial index entries are created during clone and pull
  $ cd ..
  $ hg clone -q ssh://user@dummy/repo cloned
  $ cd cloned
  $ ls .hg/store/partialindex
  b7
  $ cd ../repo
  $ mkcommit fromserver
  $ hg log -r . -T '{node}\n'
  3dd368d533d16f6172e27321f05f9a419ca354bf
  $ cd ../cloned
  $ hg pull -q
  $ ls .hg/store/partialindex
  3d
  b7
  $ hg debugprintpartialindexfile 3d
  3dd368d533d16f6172e27321f05f9a419ca354bf 1

Remove partial index and make sure everything still works
  $ rm -r .hg/store/partialindex
  $ hg log -r 3dd368
  changeset:   1:3dd368d533d1
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     fromserver
  
  $ hg debugrebuildpartialindex

Test strip
  $ hg log --graph
  o  changeset:   1:3dd368d533d1
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     fromserver
  |
  @  changeset:   0:b75a450e74d5
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     first
  
  $ hg strip -r 1
  saved backup bundle to $TESTTMP/cloned/.hg/strip-backup/3dd368d533d1-aec0bb31-backup.hg (glob)
  $ hg log --graph
  @  changeset:   0:b75a450e74d5
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     first
  
  $ hg debugcheckpartialindex

Try to commit after partial index was stripped
  $ mkcommit afterstrip
  $ hg debugcheckpartialindex

Test histedit
  $ mkcommit tohistedit
  $ hg log --graph
  @  changeset:   2:353c4093de9e
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     tohistedit
  |
  o  changeset:   1:51e0111a3ca1
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     afterstrip
  |
  o  changeset:   0:b75a450e74d5
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     first
  
  $ hg histedit --commands - <<EOF
  > pick 353c4093de9e 2 tohistedit
  > pick 51e0111a3ca1 1 afterstrip
  > EOF
  saved backup bundle to $TESTTMP/cloned/.hg/strip-backup/51e0111a3ca1-ae8f0808-backup.hg (glob)
  $ hg log --graph
  @  changeset:   2:8dc08dfc2ed8
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     afterstrip
  |
  o  changeset:   1:1d6b19400cfd
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     tohistedit
  |
  o  changeset:   0:b75a450e74d5
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     first
  
  $ ls .hg/store/partialindex
  1d
  8d
  b7

Abort strip and then restore. Check partial index
  $ rm -rf .hg/strip-backup/*
  $ printf '\n[hooks]\npriority.pretxnclose.fastpartialmatch=10' >> .hg/hgrc
  $ hg strip -r . --config hooks.pretxnclose.abort=false
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/cloned/.hg/strip-backup/8dc08dfc2ed8-5905654b-backup.hg (glob)
  transaction abort!
  rollback completed
  strip failed, backup bundle stored in '$TESTTMP/cloned/.hg/strip-backup/8dc08dfc2ed8-5905654b-backup.hg'
  abort: pretxnclose.abort hook exited with status 1
  [255]
  $ hg log --graph
  @  changeset:   1:1d6b19400cfd
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     tohistedit
  |
  o  changeset:   0:b75a450e74d5
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     first
  
  $ hg debugcheckpartialindex
  $ hg unbundle -q .hg/strip-backup/*
  $ hg log --graph
  o  changeset:   2:8dc08dfc2ed8
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     afterstrip
  |
  @  changeset:   1:1d6b19400cfd
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     tohistedit
  |
  o  changeset:   0:b75a450e74d5
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     first
  
  $ hg debugcheckpartialindex

Abort a commit
  $ echo 1 > 1
  $ hg add 1
  $ hg commit -m 'commit to discard' --config hooks.pretxnclose.abort=false
  transaction abort!
  rollback completed
  abort: pretxnclose.abort hook exited with status 1
  [255]
  $ hg forget 1
  $ rm 1
  $ hg debugcheckpartialindex

Make clone with fastpartialmatch disabled. Make pull, make sure partial index
is rebuilt
  $ cd ..
  $ hg clone --config extensions.fastpartialmatch=! -q ssh://user@dummy/repo cloned2
  $ cd repo
  $ mkcommit newcommit
  $ cd ../cloned2
  $ hg pull -q
  $ hg log --graph -T '{node}'
  o  64905857be49e6c1134f69f9743ecf8ac04a93e2
  |
  @  3dd368d533d16f6172e27321f05f9a419ca354bf
  |
  o  b75a450e74d5a7708da8c3144fbeb4ac88694044
  
  $ hg debugcheckpartialindex

Now crash during pull
  $ cd ..
  $ rm -rf cloned2
  $ hg clone -q ssh://user@dummy/repo cloned2
  $ cd repo
  $ mkcommit secondcrashpull
  $ hg log -r . -T '{node}\n'
  ac536ed8bde0682e30bb64c64570758903ce1aa6
  $ cd ../cloned2
  $ hg debugcheckpartialindex
  $ hg pull -q --config hooks.pretxnclose.abort=false
  transaction abort!
  rollback completed
  abort: pretxnclose.abort hook exited with status 1
  [255]
  $ hg debugprintpartialindexfile ac
  $ hg pull -q
  $ hg debugprintpartialindexfile ac
  ac536ed8bde0682e30bb64c64570758903ce1aa6 3
  $ hg log --graph -T '{node} {rev}'
  o  ac536ed8bde0682e30bb64c64570758903ce1aa6 3
  |
  @  64905857be49e6c1134f69f9743ecf8ac04a93e2 2
  |
  o  3dd368d533d16f6172e27321f05f9a419ca354bf 1
  |
  o  b75a450e74d5a7708da8c3144fbeb4ac88694044 0
  
  $ hg debugresolvepartialhash ac536e
  ac536e: ac536ed8bde0682e30bb64c64570758903ce1aa6 3

Test usebisect option
  $ hg debugrebuildpartialindex
  $ hg --config fastpartialmatch.usebisect=False debugresolvepartialhash ac536e
  ac536e: ac536ed8bde0682e30bb64c64570758903ce1aa6 3
