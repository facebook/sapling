  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > histedit=
  > strip=
  > EOF

  $ initrepo ()
  > {
  >     hg init r
  >     cd r
  >     for x in a b c d e f g; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  > }

  $ initrepo

log before edit
  $ hg log --graph
  @  changeset:   6:3c6a8ed2ebe8
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     g
  |
  o  changeset:   5:652413bf663e
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:e860deea161a
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:055a42cdd887
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:177f92b77385
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

edit the history
  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > edit e860deea161a e
  > pick 652413bf663e f
  > pick 3c6a8ed2ebe8 g
  > EOF
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.

edit the plan via the editor
  $ cat >> $TESTTMP/editplan.sh <<EOF
  > cat > \$1 <<EOF2
  > drop e860deea161a e
  > drop 652413bf663e f
  > drop 3c6a8ed2ebe8 g
  > EOF2
  > EOF
  $ HGEDITOR="sh $TESTTMP/editplan.sh" hg histedit --edit-plan
  $ cat .hg/histedit-state
  v1
  055a42cdd88768532f9cf79daa407fc8d138de9b
  3c6a8ed2ebe862cc949d2caa30775dd6f16fb799
  False
  3
  drop
  e860deea161a2f77de56603b340ebbb4536308ae
  drop
  652413bf663ef2a641cab26574e46d5f5a64a55a
  drop
  3c6a8ed2ebe862cc949d2caa30775dd6f16fb799
  0
  strip-backup/177f92b77385-0ebe6a8f-histedit.hg

edit the plan via --commands
  $ hg histedit --edit-plan --commands - 2>&1 << EOF
  > edit e860deea161a e
  > pick 652413bf663e f
  > drop 3c6a8ed2ebe8 g
  > EOF
  $ cat .hg/histedit-state
  v1
  055a42cdd88768532f9cf79daa407fc8d138de9b
  3c6a8ed2ebe862cc949d2caa30775dd6f16fb799
  False
  3
  edit
  e860deea161a2f77de56603b340ebbb4536308ae
  pick
  652413bf663ef2a641cab26574e46d5f5a64a55a
  drop
  3c6a8ed2ebe862cc949d2caa30775dd6f16fb799
  0
  strip-backup/177f92b77385-0ebe6a8f-histedit.hg

Go at a random point and try to continue

  $ hg id -n
  3+
  $ hg up 0
  abort: histedit in progress
  (use 'hg histedit --continue' or 'hg histedit --abort')
  [255]

Try to delete necessary commit
  $ hg strip -r 652413b
  abort: histedit in progress, can't strip 652413bf663e
  [255]

commit, then edit the revision
  $ hg ci -m 'wat'
  created new head
  $ echo a > e

qnew should fail while we're in the middle of the edit step

  $ hg --config extensions.mq= qnew please-fail
  abort: histedit in progress
  (use 'hg histedit --continue' or 'hg histedit --abort')
  [255]
  $ HGEDITOR='echo foobaz > ' hg histedit --continue 2>&1 | fixbundle
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log --graph
  @  changeset:   6:b5f70786f9b0
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   5:a5e1ba2f7afb
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     foobaz
  |
  o  changeset:   4:1a60820cd1f6
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     wat
  |
  o  changeset:   3:055a42cdd887
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:177f92b77385
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

  $ hg cat e
  a

Stripping necessary commits should not break --abort

  $ hg histedit 1a60820cd1f6 --commands - 2>&1 << EOF| fixbundle
  > edit 1a60820cd1f6 wat
  > pick a5e1ba2f7afb foobaz
  > pick b5f70786f9b0 g
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.

  $ mv .hg/histedit-state .hg/histedit-state.bak
  $ hg strip -q -r b5f70786f9b0
  $ mv .hg/histedit-state.bak .hg/histedit-state
  $ hg histedit --abort
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r .
  changeset:   6:b5f70786f9b0
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

check histedit_source

  $ hg log --debug --rev 5
  changeset:   5:a5e1ba2f7afb899ef1581cea528fd885d2fca70d
  phase:       draft
  parent:      4:1a60820cd1f6004a362aa622ebc47d59bc48eb34
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    5:5ad3be8791f39117565557781f5464363b918a45
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       e
  extra:       branch=default
  extra:       histedit_source=e860deea161a2f77de56603b340ebbb4536308ae
  description:
  foobaz
  
  

  $ hg histedit tip --commands - 2>&1 <<EOF| fixbundle
  > edit b5f70786f9b0 f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.
  $ hg status
  A f

  $ hg summary
  parent: 5:a5e1ba2f7afb 
   foobaz
  branch: default
  commit: 1 added (new branch head)
  update: 1 new changesets (update)
  phases: 7 draft
  hist:   1 remaining (histedit --continue)

(test also that editor is invoked if histedit is continued for
"edit" action)

  $ HGEDITOR='cat' hg histedit --continue
  f
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added f
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/b5f70786f9b0-c28d9c86-backup.hg (glob)

  $ hg status

log after edit
  $ hg log --limit 1
  changeset:   6:a107ee126658
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

say we'll change the message, but don't.
  $ cat > ../edit.sh <<EOF
  > cat "\$1" | sed s/pick/mess/ > tmp
  > mv tmp "\$1"
  > EOF
  $ HGEDITOR="sh ../edit.sh" hg histedit tip 2>&1 | fixbundle
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  $ hg log --limit 1
  changeset:   6:1fd3b2fe7754
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

modify the message

check saving last-message.txt, at first

  $ cat > $TESTTMP/commitfailure.py <<EOF
  > from mercurial import util
  > def reposetup(ui, repo):
  >     class commitfailure(repo.__class__):
  >         def commit(self, *args, **kwargs):
  >             raise util.Abort('emulating unexpected abort')
  >     repo.__class__ = commitfailure
  > EOF
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > # this failure occurs before editor invocation
  > commitfailure = $TESTTMP/commitfailure.py
  > EOF

  $ cat > $TESTTMP/editor.sh <<EOF
  > echo "==== before editing"
  > cat \$1
  > echo "===="
  > echo "check saving last-message.txt" >> \$1
  > EOF

(test that editor is not invoked before transaction starting)

  $ rm -f .hg/last-message.txt
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg histedit tip --commands - 2>&1 << EOF | fixbundle
  > mess 1fd3b2fe7754 f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  abort: emulating unexpected abort
  $ test -f .hg/last-message.txt
  [1]

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > commitfailure = !
  > EOF
  $ hg histedit --abort -q

(test that editor is invoked and commit message is saved into
"last-message.txt")

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > # this failure occurs after editor invocation
  > pretxncommit.unexpectedabort = false
  > EOF

  $ hg status --rev '1fd3b2fe7754^1' --rev 1fd3b2fe7754
  A f

  $ rm -f .hg/last-message.txt
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg histedit tip --commands - 2>&1 << EOF
  > mess 1fd3b2fe7754 f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  adding f
  ==== before editing
  f
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added f
  ====
  transaction abort!
  rollback completed
  note: commit message saved in .hg/last-message.txt
  abort: pretxncommit.unexpectedabort hook exited with status 1
  [255]
  $ cat .hg/last-message.txt
  f
  
  
  check saving last-message.txt

(test also that editor is invoked if histedit is continued for "message"
action)

  $ HGEDITOR=cat hg histedit --continue
  f
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added f
  transaction abort!
  rollback completed
  note: commit message saved in .hg/last-message.txt
  abort: pretxncommit.unexpectedabort hook exited with status 1
  [255]

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > pretxncommit.unexpectedabort =
  > EOF
  $ hg histedit --abort -q

then, check "modify the message" itself

  $ hg histedit tip --commands - 2>&1 << EOF | fixbundle
  > mess 1fd3b2fe7754 f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  $ hg log --limit 1
  changeset:   6:62feedb1200e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

rollback should not work after a histedit
  $ hg rollback
  no rollback information available
  [1]

  $ cd ..
  $ hg clone -qr0 r r0
  $ cd r0
  $ hg phase -fdr0
  $ hg histedit --commands - 0 2>&1 << EOF
  > edit cb9a9f314b8b a > $EDITED
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  adding a
  Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.
  [1]
  $ HGEDITOR=true hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/r0/.hg/strip-backup/cb9a9f314b8b-cc5ccb0b-backup.hg (glob)

  $ hg log -G
  @  changeset:   0:0efcea34f18a
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
  $ echo foo >> b
  $ hg addr
  adding b
  $ hg ci -m 'add b'
  $ echo foo >> a
  $ hg ci -m 'extend a'
  $ hg phase --public 1
Attempting to fold a change into a public change should not work:
  $ cat > ../edit.sh <<EOF
  > cat "\$1" | sed s/pick/fold/ > tmp
  > mv tmp "\$1"
  > EOF
  $ HGEDITOR="sh ../edit.sh" hg histedit 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  reverting a
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  abort: cannot fold into public change 18aa70c8ad22
  [255]
TODO: this abort shouldn't be required, but it is for now to leave the repo in
a clean state.
  $ hg histedit --abort
