
  $ mkcommit() {
  >  echo "$1" > "$1"
  >  hg add "$1"
  >  hg ci -m "$1"
  > }

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastpartialmatch=$TESTDIR/../hgext3rd/fastpartialmatch.py
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
  $ ls -al .hg/store/partialindex/ | sort
  -rw-r--r--.* generationnum (re)
  -rw-r--r--.* b7 (re)
  drwxr-xr-x.* \. (re)
  drwxr-xr-x.* \.\. (re)
  total \d+ (re)

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
  $ ls .hg/store/partialindex | sort
  b7
  generationnum
  $ cd ../repo
  $ mkcommit fromserver
  $ hg log -r . -T '{node}\n'
  3dd368d533d16f6172e27321f05f9a419ca354bf
  $ cd ../cloned
  $ hg pull -q
  $ ls .hg/store/partialindex | sort
  3d
  b7
  generationnum
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
  saved backup bundle to $TESTTMP/cloned/.hg/strip-backup/51e0111a3ca1-ae8f0808-histedit.hg (glob)
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
  
  $ ls .hg/store/partialindex | sort
  1d
  8d
  b7
  generationnum

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

Test cache rebuilding
  $ cd ../repo
  $ mkcommit committotriggercacherebuilding
  $ cd ../cloned2
  $ printf '[fastpartialmatch]\nunsortedthreshold=1' >> .hg/hgrc
  $ hg up -q ac536ed8bde0
  $ [ -f .hg/partialindexneedrebuild ]
  [1]
  $ mkcommit commit
  $ hg log -r . -T '{node}\n'
  587cd78c6d0eb0259484b09a5983bcc2973f2245

Next command should set that cache needs rebuilding
  $ hg log -r 587cd78c6d0eb0259 > /dev/null
  $ [ -f .hg/partialindexneedrebuild ]
  $ hg debugfastpartialmatchstat
  generation number: 0
  index will be rebuilt on the next pull
  file: 3d, entries: 1, out of them 1 sorted
  file: 58, entries: 1, out of them 0 sorted
  file: 64, entries: 1, out of them 1 sorted
  file: ac, entries: 1, out of them 1 sorted
  file: b7, entries: 1, out of them 1 sorted

Now do a pull and make sure that index was rebuilt (file '12' is not rebuilt
because it was just pulled)
  $ hg pull -q
  $ hg debugfastpartialmatchstat
  generation number: 0
  file: 12, entries: 1, out of them 0 sorted
  file: 3d, entries: 1, out of them 1 sorted
  file: 58, entries: 1, out of them 1 sorted
  file: 64, entries: 1, out of them 1 sorted
  file: ac, entries: 1, out of them 1 sorted
  file: b7, entries: 1, out of them 1 sorted

Increase unsortedthreshold and make one more pull. Make sure index doesn't need
to be rebuilt
  $ cd ../repo
  $ mkcommit somecommit
  $ cd ../cloned2
  $ hg pull -q --config fastpartialmatch.unsortedthreshold=2
  $ hg debugfastpartialmatchstat
  generation number: 0
  file: 12, entries: 1, out of them 0 sorted
  file: 3d, entries: 1, out of them 1 sorted
  file: 58, entries: 1, out of them 1 sorted
  file: 64, entries: 1, out of them 1 sorted
  file: ac, entries: 1, out of them 1 sorted
  file: b7, entries: 1, out of them 1 sorted
  file: fd, entries: 1, out of them 0 sorted

Make a commit and change .hg permissions to non-writabble. Then do
partial lookup that should write needrebuild file but it couldn't because
of permissions. Make sure it doesn't throw and just log the problem
  $ mkcommit commitpermissionissue
  $ chmod u-w .hg/
  $ hg log -r . -T '{node}\n'
  2b52832374dd7e499a1fbd172f1d75e13ee32477
  $ hg log -r 2b5283237
  changeset:   7:2b52832374dd
  tag:         tip
  parent:      4:587cd78c6d0e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commitpermissionissue
  
  error happened while triggering rebuild: [Errno 13] Permission denied: '$TESTTMP/cloned2/.hg/partialindexneedrebuild'
  $ chmod u+w .hg/

Temporarily disable the fastpartialmatch and make a commit.
Bump index generation number version and check that index was deleted after next
invocation

  $ echo disabled > disabled
  $ hg add disabled
  $ hg --config extensions.fastpartialmatch=! ci -m disabled
  $ hg debugcheckpartialindex
  e26f27bfb47a8eb9df0ee8f0feeb850722bc416d node not found in partialindex
  [1]
  $ printf "\n[fastpartialmatch]\ngenerationnumber=1\n" >> .hg/hgrc
  $ hg log -r . -T '{node}'
  e26f27bfb47a8eb9df0ee8f0feeb850722bc416d (no-eol)
  $ hg debugcheckpartialindex
  partial index is not built
  [1]

Make pull to rebuild the index
  $ cd ../repo
  $ mkcommit servercommit
  $ cd ../cloned2
  $ hg pull -q
  $ hg debugcheckpartialindex
  $ hg debugfastpartialmatchstat
  generation number: 1
  file: 06, entries: 1, out of them 0 sorted
  file: 12, entries: 1, out of them 1 sorted
  file: 2b, entries: 1, out of them 1 sorted
  file: 3d, entries: 1, out of them 1 sorted
  file: 58, entries: 1, out of them 1 sorted
  file: 64, entries: 1, out of them 1 sorted
  file: ac, entries: 1, out of them 1 sorted
  file: b7, entries: 1, out of them 1 sorted
  file: e2, entries: 1, out of them 1 sorted
  file: fd, entries: 1, out of them 1 sorted

Write incorrect generation number
  $ echo badgennum > .hg/store/partialindex/generationnum
  $ hg log -r .
  error happened while reading generation num: invalid literal for int() with base 10: 'badgennum\n'
  changeset:   8:e26f27bfb47a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     disabled
  
  $ hg debugcheckpartialindex
  partial index is not built
  [1]

Create bookmark with the same prefix as commit hash. hg log should show commit
with bookmark
  $ hg debugrebuildpartialindex
  $ hg book e26f27bf
  bookmark e26f27bf matches a changeset hash
  (did you leave a -r out of an 'hg bookmark' command?)
  $ mkcommit newcommitwithbook
  $ hg log -r e26f27bf
  changeset:   10:bf72b6cd3f5a
  bookmark:    e26f27bf
  tag:         tip
  parent:      8:e26f27bfb47a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     newcommitwithbook
  
Do amend and check index
  $ hg debugrebuildpartialindex
  $ mkcommit toamend
  $ echo amended > toamend
  $ hg ci -m amended --amend -q
  $ hg debugcheckpartialindex

Try to create empty commit
  $ hg ci -m empty
  nothing changed
  [1]

Try to checkout nullid
  $ hg up --config fastpartialmatch.raiseifinconsistent=True -q 0000000000
