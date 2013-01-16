  $ . "$TESTDIR/histedit-helpers.sh"

Enable obsolete

  $ cat > ${TESTTMP}/obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate= {rev}:{node|short} {desc|firstline}
  > [phases]
  > publish=False
  > [extensions]'
  > histedit=
  > rebase=
  > 
  > obs=${TESTTMP}/obs.py
  > EOF

  $ hg init base
  $ cd base

  $ for x in a b c d e f ; do
  >     echo $x > $x
  >     hg add $x
  >     hg ci -m $x
  > done

  $ hg log --graph
  @  5:652413bf663e f
  |
  o  4:e860deea161a e
  |
  o  3:055a42cdd887 d
  |
  o  2:177f92b77385 c
  |
  o  1:d2ae7f538514 b
  |
  o  0:cb9a9f314b8b a
  

  $ HGEDITOR=cat hg histedit 1
  pick d2ae7f538514 1 b
  pick 177f92b77385 2 c
  pick 055a42cdd887 3 d
  pick e860deea161a 4 e
  pick 652413bf663e 5 f
  
  # Edit history between d2ae7f538514 and 652413bf663e
  #
  # Commands:
  #  p, pick = use commit
  #  e, edit = use commit, but stop for amending
  #  f, fold = use commit, but fold into previous commit (combines N and N-1)
  #  d, drop = remove commit from history
  #  m, mess = edit message without changing commit content
  #
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat > commands.txt <<EOF
  > pick 177f92b77385 2 c
  > drop d2ae7f538514 1 b
  > pick 055a42cdd887 3 d
  > fold e860deea161a 4 e
  > pick 652413bf663e 5 f
  > EOF
  $ hg histedit 1 --commands commands.txt --verbose | grep histedit
  saved backup bundle to $TESTTMP/base/.hg/strip-backup/96e494a2d553-backup.hg (glob)
  $ hg log --graph --hidden
  @  8:cacdfd884a93 f
  |
  o  7:59d9f330561f d
  |
  o  6:b346ab9a313d c
  |
  | x  5:652413bf663e f
  | |
  | x  4:e860deea161a e
  | |
  | x  3:055a42cdd887 d
  | |
  | x  2:177f92b77385 c
  | |
  | x  1:d2ae7f538514 b
  |/
  o  0:cb9a9f314b8b a
  
  $ hg debugobsolete
  d2ae7f538514cd87c17547b0de4cea71fe1af9fb 0 {'date': '* *', 'user': 'test'} (glob)
  177f92b773850b59254aa5e923436f921b55483b b346ab9a313db8537ecf96fca3ca3ca984ef3bd7 0 {'date': '* *', 'user': 'test'} (glob)
  055a42cdd88768532f9cf79daa407fc8d138de9b 59d9f330561fd6c88b1a6b32f0e45034d88db784 0 {'date': '* *', 'user': 'test'} (glob)
  e860deea161a2f77de56603b340ebbb4536308ae 59d9f330561fd6c88b1a6b32f0e45034d88db784 0 {'date': '* *', 'user': 'test'} (glob)
  652413bf663ef2a641cab26574e46d5f5a64a55a cacdfd884a9321ec4e1de275ef3949fa953a1f83 0 {'date': '* *', 'user': 'test'} (glob)


Ensure hidden revision does not prevent histedit
-------------------------------------------------

create an hidden revision

  $ cat > commands.txt <<EOF
  > pick b346ab9a313d 6 c
  > drop 59d9f330561f 7 d
  > pick cacdfd884a93 8 f
  > EOF
  $ hg histedit 6 --commands commands.txt
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log --graph
  @  9:c13eb81022ca f
  |
  o  6:b346ab9a313d c
  |
  o  0:cb9a9f314b8b a
  
check hidden revision are ignored (6 have hidden children 7 and 8)

  $ cat > commands.txt <<EOF
  > pick b346ab9a313d 6 c
  > pick c13eb81022ca 8 f
  > EOF
  $ hg histedit 6 --commands commands.txt
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved


Check that histedit respect phases
=========================================

(not directly related to the test file but doesn't deserve it's own test case)

  $ hg log -G
  @  9:c13eb81022ca f
  |
  o  6:b346ab9a313d c
  |
  o  0:cb9a9f314b8b a
  
  $ hg ph -pv '.^'
  phase changed for 2 changesets
  $ hg histedit -r '.~2'
  abort: cannot edit immutable changeset: cb9a9f314b8b
  [255]


Test that rewriting leaving instability behind is allowed
---------------------------------------------------------------------

  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg ph --force --draft '.'
  $ hg log -r 'children(.)'
  9:c13eb81022ca f (no-eol)
  $ cat > commands.txt <<EOF
  > edit b346ab9a313d 6 c
  > EOF
  $ hg histedit -r '.' --commands commands.txt
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  adding c
  abort: Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.
  [255]
  $ echo c >> c
  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -r 'unstable()'
  9:c13eb81022ca f (no-eol)

stabilise

  $ hg rebase  -r 'unstable()' -d .
