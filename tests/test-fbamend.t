Test functionality is present

  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/fbamend.py $TESTTMP # use $TESTTMP substitution in message
  $ cp $extpath/fbhistedit.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=$TESTTMP/fbamend.py
  > rebase=
  > EOF
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    hg ci -l msg
  > }


  $ hg help commit | grep -- --fixup
      --fixup               (with --amend) rebase children commits from a
  $ hg help commit | grep -- --rebase
      --rebase              (with --amend) rebases children commits after the
  $ hg help amend
  hg amend [OPTION]...
  
  amend the current commit with more changes
  
  options ([+] can be repeated):
  
   -A --addremove           mark new/missing files as added/removed before
                            committing
   -e --edit                prompt to edit the commit message
   -i --interactive         use interactive mode
      --rebase              rebases children commits after the amend
      --fixup               rebase children commits from a previous amend
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -m --message TEXT        use text as commit message
   -l --logfile FILE        read commit message from file
  
  (some details hidden, use --verbose to show complete help)

Test basic functions

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg commit -m 'a'
  $ echo a >> a
  $ hg commit -m 'aa'
  $ echo b >> b
  $ hg add b
  $ hg commit -m 'b'
  $ hg up ".^"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a >> a
  $ hg amend
  warning: the commit's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo x >> x
  $ hg commit -Am 'extra commit to test multiple heads'
  adding x
  created new head
  $ hg up 3
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg amend --fixup
  rebasing the children of 34414ab6546d.preamend
  rebasing 2:a764265b74cf "b"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/a764265b74cf-c5eef4f8-backup.hg (glob)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/86cf3bb05fcf-36a6cbd7-preamend-backup.hg (glob)
  $ echo a >> a
  $ hg amend --rebase
  rebasing the children of 7817096bf624.preamend
  rebasing 3:e1c831172263 "b"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/e1c831172263-eee3b8f6-backup.hg (glob)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/34414ab6546d-72d06a8e-preamend-backup.hg (glob)

Test that current bookmark is maintained

  $ hg bookmark bm
  $ hg bookmarks
   * bm                        2:7817096bf624
  $ echo a >> a
  $ hg amend --rebase
  rebasing the children of bm.preamend
  rebasing 3:1e390e3ec656 "b"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/1e390e3ec656-8362bab7-backup.hg (glob)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/7817096bf624-d72fddeb-preamend-backup.hg (glob)
  $ hg bookmarks
   * bm                        2:7635008c16e1

Set up education

  $ echo "[fbamend]" >> $HGRCPATH
  $ echo "education = user education" >> $HGRCPATH
  $ echo "    second line" >> $HGRCPATH
  $ echo "" >> $HGRCPATH

Test that bookmarked re-amends work well

  $ echo a >> a
  $ hg amend
  user education
  second line
  warning: the commit's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg log -G -T '{node|short} {desc} {bookmarks}\n'
  @  edf5fd2f5332 aa bm
  |
  | o  2d6884e15790 b
  | |
  | o  7635008c16e1 aa bm.preamend
  |/
  | o  3f6197b00eba extra commit to test multiple heads
  |/
  o  cb9a9f314b8b a
  
  $ echo a >> a
  $ hg amend
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/edf5fd2f5332-81b0ec5b-amend-backup.hg (glob)
  $ hg log -G -T '{node|short} {desc} {bookmarks}\n'
  @  0889a0030a17 aa bm
  |
  | o  2d6884e15790 b
  | |
  | o  7635008c16e1 aa bm.preamend
  |/
  | o  3f6197b00eba extra commit to test multiple heads
  |/
  o  cb9a9f314b8b a
  
  $ hg amend --fixup
  rebasing the children of bm.preamend
  rebasing 3:2d6884e15790 "b"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/2d6884e15790-909076cb-backup.hg (glob)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/7635008c16e1-65f65ff6-preamend-backup.hg (glob)
  $ hg log -G -T '{node|short} {desc} {bookmarks}\n'
  o  6ba7926ba204 b
  |
  @  0889a0030a17 aa bm
  |
  | o  3f6197b00eba extra commit to test multiple heads
  |/
  o  cb9a9f314b8b a
  
  $ hg bookmarks
   * bm                        2:0889a0030a17

Test that unbookmarked re-amends work well

  $ hg boo -d bm
  $ echo a >> a
  $ hg amend
  user education
  second line
  warning: the commit's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg log -G -T '{node|short} {desc} {bookmarks}\n'
  @  94eb429c9465 aa
  |
  | o  6ba7926ba204 b
  | |
  | o  0889a0030a17 aa 94eb429c9465.preamend
  |/
  | o  3f6197b00eba extra commit to test multiple heads
  |/
  o  cb9a9f314b8b a
  
  $ echo a >> a
  $ hg amend
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/94eb429c9465-30a7ee2c-amend-backup.hg (glob)
  $ hg log -G -T '{node|short} {desc} {bookmarks}\n'
  @  83455f1f6049 aa
  |
  | o  6ba7926ba204 b
  | |
  | o  0889a0030a17 aa 83455f1f6049.preamend
  |/
  | o  3f6197b00eba extra commit to test multiple heads
  |/
  o  cb9a9f314b8b a
  
  $ hg amend --fixup
  rebasing the children of 83455f1f6049.preamend
  rebasing 3:6ba7926ba204 "b"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/6ba7926ba204-9ac223ef-backup.hg (glob)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/0889a0030a17-6bebea0c-preamend-backup.hg (glob)
  $ hg log -G -T '{node|short} {desc} {bookmarks}\n'
  o  455e4104f605 b
  |
  @  83455f1f6049 aa
  |
  | o  3f6197b00eba extra commit to test multiple heads
  |/
  o  cb9a9f314b8b a
  

Test interaction with histedit

  $ echo '[extensions]' >> $HGRCPATH
  $ echo "fbhistedit=$TESTTMP/fbhistedit.py" >> $HGRCPATH
  $ echo "histedit=" >> $HGRCPATH
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> c
  $ hg add c
  $ hg commit -m c
  $ hg log -T '{node|short} {desc}\n'
  765b28efbe8b c
  455e4104f605 b
  83455f1f6049 aa
  3f6197b00eba extra commit to test multiple heads
  cb9a9f314b8b a
  $ hg histedit ".^^" --commands - <<EOF
  > pick 83455f1f6049
  > x echo amending from exec
  > x hg commit --amend -m 'message from exec'
  > stop 455e4104f605
  > pick 765b28efbe8b
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  amending from exec
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: the commit's children were left behind
  (this is okay since a histedit is in progress)
  Changes commited as a2329fab3fab. You may amend the commit now.
  When you are done, run hg histedit --continue to resume
  [1]
  $ hg log -G -T '{node|short} {desc} {bookmarks}\n'
  @  a2329fab3fab b
  |
  o  048e86baa19d message from exec
  |
  | o  765b28efbe8b c
  | |
  | o  455e4104f605 b
  | |
  | o  83455f1f6049 aa
  |/
  | o  3f6197b00eba extra commit to test multiple heads
  |/
  o  cb9a9f314b8b a
  
  $ hg amend --rebase
  abort: histedit in progress
  (during histedit, use amend without --rebase)
  [255]
  $ hg commit --amend -m 'commit --amend message'
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/a2329fab3fab-e6fb940f-amend-backup.hg (glob)
  $ hg log -G -T '{node|short} {desc} {bookmarks}\n'
  @  3166f3b5587d commit --amend message
  |
  o  048e86baa19d message from exec
  |
  | o  765b28efbe8b c
  | |
  | o  455e4104f605 b
  | |
  | o  83455f1f6049 aa
  |/
  | o  3f6197b00eba extra commit to test multiple heads
  |/
  o  cb9a9f314b8b a
  
  $ hg histedit --continue
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/83455f1f6049-922a304e-backup.hg (glob)
  $ hg log -G -T '{node|short} {desc} {bookmarks}\n'
  @  0f83a9508203 c
  |
  o  3166f3b5587d commit --amend message
  |
  o  048e86baa19d message from exec
  |
  | o  3f6197b00eba extra commit to test multiple heads
  |/
  o  cb9a9f314b8b a
  
Test that --message is respected

  $ hg amend
  nothing changed
  $ hg amend --message foo
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/0f83a9508203-7d2a99ee-amend-backup.hg (glob)
  $ hg amend -m bar
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/29272a1da891-35a82ce4-amend-backup.hg (glob)
  $ hg amend
  nothing changed

Test that --addremove/-A works

  $ echo new > new
  $ hg amend -A
  adding new
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/772f45f5a69d-90a7bd63-amend-backup.hg (glob)

Test that the extension disables itself when evolution is enabled

  $ $PYTHON -c 'import evolve' 2> /dev/null || $PYTHON -c 'import hgext.evolve' 2> /dev/null || exit 80
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > evolve=
  > EOF

noisy warning during amend

  $ hg amend 2>&1
  fbamend and evolve extension are incompatible, fbamend deactivated.
  You can either disable it globally:
  - type `hg config --edit`
  - drop the `fbamend=` line from the `[extensions]` section
  or disable it for a specific repo:
  - type `hg config --local --edit`
  - add a `fbamend=!$TESTTMP/fbamend.py` line in the `[extensions]` section
  nothing changed

no warning if only obsolete markers are enabled

  $ cat >> .hg/hgrc <<EOF
  > [experimental]
  > evolution=createmarkers
  > EOF

  $ hg amend
  nothing changed

Fbamend respects the createmarkers option

  $ hg log -G -T '{rev} {node|short} {desc} {bookmarks}\n'
  @  4 01dd7a39383a bar
  |
  o  3 3166f3b5587d commit --amend message
  |
  o  2 048e86baa19d message from exec
  |
  | o  1 3f6197b00eba extra commit to test multiple heads
  |/
  o  0 cb9a9f314b8b a
  
  $ hg up 048e86
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo "bb" > bb
  $ hg add bb
  $ hg amend --debug
  amending changeset 048e86baa19d
  committing files:
  bb
  committing manifest
  committing changelog
  copying changeset 4e21ff9ac40b to cb9a9f314b8b
  committing files:
  a
  bb
  committing manifest
  committing changelog
  user education
  second line
  warning: the commit's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg amend --fixup
  rebasing the children of 3a4d2824efc1.preamend
  rebasing 3:3166f3b5587d "commit --amend message"
  rebasing 4:01dd7a39383a "bar"
  $ hg log -G -T '{rev} {node|short} {desc} {bookmarks}\n'
  o  8 9752120dcffe bar
  |
  o  7 bc3b6a46cdb4 commit --amend message
  |
  @  6 3a4d2824efc1 message from exec
  |
  | o  1 3f6197b00eba extra commit to test multiple heads
  |/
  o  0 cb9a9f314b8b a
  
  $ echo "cc" > cc
  $ hg add cc
  $ hg amend --rebase --traceback
  rebasing the children of 1c16dd8e35d2.preamend
  rebasing 7:bc3b6a46cdb4 "commit --amend message"
  rebasing 8:9752120dcffe "bar"

Test that fbamend works with interactive commits (crecord)
  $ cat >> $HGRCPATH << EOF
  > [ui]
  > interactive = true
  > interface = curses
  > [experimental]
  > crecordtest = testModeCommands
  > EOF
  $ hg up 18b434184b29 -q
  $ echo 'This line will remain into the next amend' > afile
  $ hg add afile
  $ hg commit -m 'commit to be amended'
  $ echo "some content" > afile

test hg amend -i works in a stack
  $ hg commit -m 'descendant commit'
  $ hg up ".^" -q -C
  $ echo 'be happy' > anotherfile
  $ hg add anotherfile
Just commit the file in interactive mode
  $ cat <<EOF > testModeCommands
  > X
  > EOF
  $ hg amend -i --config "fbamend.education=" -q
  warning: the commit's children were left behind
preamend bookmark exists
  $ hg log -G -T '{bookmarks}' | grep 'preamend'
  | x  6cd3bb4b4ada.preamend
Make sure fixup gets rid of preamend bookmarks (there should be none)
  $ hg amend --fixup
  rebasing the children of 6cd3bb4b4ada.preamend
  rebasing 14:ab75b93512f7 "descendant commit"
preamend bookmark has been removed
  $ hg log -G -T '{bookmarks}' | grep 'preamend'
  [1]

test hg commit -i --amend works in a stack
  $ echo 'be honest' > anotherfile
  $ cat <<EOF > testModeCommands
  > X
  > EOF
  $ hg commit -i --amend --config "fbamend.education=" -q -m 'a commit msg'
  warning: the commit's children were left behind
  1 new unstable changesets
preamend bookmark exists
  $ hg log -G -T '{bookmarks}' | grep 'preamend'
  | x  039ee914a5fd.preamend
Make sure fixup gets rid of preamend bookmarks (there should be none)
  $ hg amend --fixup
  rebasing the children of 039ee914a5fd.preamend
  rebasing 17:2c40a2aa23f1 "descendant commit"
preamend bookmark has been removed
  $ hg log -G -T '{bookmarks}' | grep 'preamend'
  [1]
  $ rm testModeCommands

Test fbamend fails with both --interactive and --rebase
  $ hg commit --amend --rebase -i
  abort: --interactive and --rebase are mutually exclusive
  [255]
  $ hg amend --rebase -i
  abort: --interactive and --rebase are mutually exclusive
  [255]

Test fbamend fails with both --interactive and --fixup
  $ hg commit --amend --fixup -i
  abort: --interactive and --fixup are mutually exclusive
  [255]
  $ hg amend --fixup -i
  abort: --interactive and --fixup are mutually exclusive
  [255]

Test commit fails with --fixup and --rebase without --amend
  $ hg commit --fixup
  abort: --fixup must be called with --amend
  [255]
  $ hg commit --rebase
  abort: --rebase must be called with --amend
  [255]

Test hg amend works with a logfile
  $ hg up -r 'last(.::)' -q
  $ echo 'content change' > a
  $ echo 'this will be a logfile based commit message' > alogfile
  $ hg amend --logfile alogfile
  $ hg log -r . -T '{desc}'
  this will be a logfile based commit message (no-eol)

  $ echo 'another content change' > a
  $ hg amend --logfile alogfile --message 'Crazy. You can not mix -m with -l'
  abort: options --message and --logfile are mutually exclusive
  [255]
  $ rm alogfile

Test fbamend with inhibit
  $ $PYTHON -c 'import inhibit' 2> /dev/null || $PYTHON -c 'import hgext.inhibit' 2> /dev/null || exit 80
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > inhibit=
  > EOF
  $ cd ..
  $ hg init inhibitrepo
  $ cd inhibitrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg log --template '{node|short} {desc}' --graph
  @  4538525df7e2 add c
  |
  o  7c3bad9141dc add b
  |
  o  1f0dee641bb7 add a
  
  $ hg up ".^" -q
  $ echo "hello" > b
  $ hg amend
  user education
  second line
  warning: the commit's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg amend --fixup
  rebasing the children of f2d4abddbbcd.preamend
  rebasing 2:4538525df7e2 "add c"
  saved backup bundle to $TESTTMP/inhibitrepo/.hg/strip-backup/4538525df7e2-0bcb0716-backup.hg (glob)
  saved backup bundle to $TESTTMP/inhibitrepo/.hg/strip-backup/7c3bad9141dc-81844e36-preamend-backup.hg (glob)
  $ hg log --template '{node|short} {desc}' --graph
  o  084836c39cc1 add c
  |
  @  f2d4abddbbcd add b
  |
  o  1f0dee641bb7 add a
  
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit d
  $ hg up ".^" -q
  $ echo "hello" > c
  $ hg amend --rebase
  rebasing the children of 6d41fcaa1aa4.preamend
  rebasing 3:3a033d20b13a "add d"
  saved backup bundle to $TESTTMP/inhibitrepo/.hg/strip-backup/3a033d20b13a-c275a49d-backup.hg (glob)
  saved backup bundle to $TESTTMP/inhibitrepo/.hg/strip-backup/084836c39cc1-e86e0471-preamend-backup.hg (glob)
  $ hg log --template '{node|short} {desc}' --graph
  o  cc9fcfa87676 add d
  |
  @  6d41fcaa1aa4 add c
  |
  o  f2d4abddbbcd add b
  |
  o  1f0dee641bb7 add a
  
  $ cd ..

Prepare a repo for unamend testing
  $ hg init unamendrepo
  $ cd unamendrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > evolve=
  > [experimental]
  > evolution=createmarkers
  > EOF
  $ echo a > a && echo b > b
  $ hg ci -Am ab
  adding a
  adding b

Create and activate a bookmark to test bookmark movement around unamend
  $ hg book -r . b1
  $ hg book -r . b2
  $ hg up b1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark b1)

See how the original commit looks
  $ hg log
  changeset:   0:b6a1406d8886
  bookmark:    b1
  bookmark:    b2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     ab
  
Amend a commit
  $ hg rm b
  $ echo c > c && hg add c
  $ echo aa > a
  $ hg status
  M a
  A c
  R b
  $ hg amend -m ab2

See how the amended commit looks
  $ hg log
  changeset:   2:551468b37da8
  bookmark:    b1
  bookmark:    b2
  tag:         tip
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     ab2
  
Make sure that unamend does not work without inhibit
  $ hg unamend
  abort: unamend requires inhibit extension to be enabled
  (please add inhibit to the list of enabled extensions)
  [255]

Make sure that unamend works as expected with inhibit
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > inhibit=
  > directaccess=
  > EOF
  $ hg unamend

See how the commit looks after unamending
  $ hg log
  changeset:   0:b6a1406d8886
  bookmark:    b1
  bookmark:    b2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     ab
  
  $ hg status
  M a
  A c
  R b

Check whether active bookmark remained active
  $ hg book
   * b1                        0:b6a1406d8886
     b2                        0:b6a1406d8886

Check whether unamend works with dirty working directory
  $ hg amend -m "bring back the amended commit"
  $ hg st
  $ hg log -r .
  changeset:   3:dd05d03a1c51
  bookmark:    b1
  bookmark:    b2
  tag:         tip
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     bring back the amended commit
  
  $ echo d > d && hg add d
  $ hg st
  A d
  $ hg unamend
  $ hg log -r .
  changeset:   0:b6a1406d8886
  bookmark:    b1
  bookmark:    b2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     ab
  
  $ hg st
  M a
  A c
  A d
  R b

