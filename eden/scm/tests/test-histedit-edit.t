#chg-compatible

  $ setconfig workingcopy.ruststatus=False
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable histedit

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
  @  commit:      3c6a8ed2ebe8
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     g
  │
  o  commit:      652413bf663e
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     f
  │
  o  commit:      e860deea161a
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     e
  │
  o  commit:      055a42cdd887
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     d
  │
  o  commit:      177f92b77385
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     c
  │
  o  commit:      d2ae7f538514
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     b
  │
  o  commit:      cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
dirty a file
  $ echo a > g
  $ hg histedit 177f92b77385 --commands - << EOF
  > EOF
  abort: uncommitted changes
  [255]
  $ echo g > g

edit the history
  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > edit e860deea161a e
  > pick 652413bf663e f
  > pick 3c6a8ed2ebe8 g
  > EOF
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  Editing (e860deea161a), you may commit or record as needed now.
  (hg histedit --continue to resume)

try to update and get an error
  $ hg goto tip
  abort: histedit in progress
  (use 'hg histedit --continue' or 'hg histedit --abort')
  [255]

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
  

edit the plan via --commands
  $ hg histedit --edit-plan --commands - << EOF
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
  

Go at a random point and try to continue

  $ hg id -n
  3+
  $ hg up 0
  abort: histedit in progress
  (use 'hg histedit --continue' or 'hg histedit --abort')
  [255]

Try to delete necessary commit
  $ hg debugstrip -r 652413b
  abort: histedit in progress, can't strip 652413bf663e
  [255]

commit, then edit the revision
  $ hg ci -m 'wat'
  $ echo a > e

  $ HGEDITOR='echo foobaz > ' hg histedit --continue 2>&1 | fixbundle

  $ hg cat e
  a

Stripping necessary commits should not break --abort
(No longer true - skipped this test since debugstrip is rarely used)

  $ hg histedit 1a60820cd1f6 --commands - 2>&1 << EOF| fixbundle
  > edit 1a60820cd1f6 wat
  > pick a5e1ba2f7afb foobaz
  > pick b5f70786f9b0 g
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  Editing (1a60820cd1f6), you may commit or record as needed now.
  (hg histedit --continue to resume)

  $ hg histedit --abort
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r .
  commit:      b5f70786f9b0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

check histedit_source

  $ hg log --debug --rev 'desc(foobaz)'
  commit:      a5e1ba2f7afb899ef1581cea528fd885d2fca70d
  phase:       draft
  manifest:    5ad3be8791f39117565557781f5464363b918a45
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
  Editing (b5f70786f9b0), you may commit or record as needed now.
  (hg histedit --continue to resume)
  $ hg status
  A f

  $ hg summary
  parent: a5e1ba2f7afb 
   foobaz
  commit: 1 added
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

  $ hg status

log after edit
  $ hg log --limit 1
  commit:      a107ee126658
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

say we'll change the message, but don't.
  $ cat > ../edit.sh <<EOF
  > cat "\$1" | sed s/pick/mess/ > tmp
  > mv tmp "\$1"
  > EOF
  $ HGEDITOR="sh ../edit.sh" hg histedit tip 2>&1 | fixbundle
  $ hg status
  $ hg log --limit 1
  commit:      1fd3b2fe7754
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

modify the message

check saving last-message.txt, at first

  $ cat > $TESTTMP/commitfailure.py <<EOF
  > from edenscm import error
  > def reposetup(ui, repo):
  >     class commitfailure(repo.__class__):
  >         def commit(self, *args, **kwargs):
  >             raise error.Abort('emulating unexpected abort')
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
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg histedit tip --commands - << EOF
  > mess 1fd3b2fe7754 f
  > EOF
  ==== before editing
  f
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added f
  ====
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
  note: commit message saved in .hg/last-message.txt
  abort: pretxncommit.unexpectedabort hook exited with status 1
  [255]

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > pretxncommit.unexpectedabort =
  > EOF
  $ hg histedit --abort -q

then, check "modify the message" itself

  $ hg histedit . --commands - << EOF | fixbundle
  > mess 1fd3b2fe7754 f
  > EOF
  $ hg status
  $ hg log --limit 1
  commit:      62feedb1200e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

