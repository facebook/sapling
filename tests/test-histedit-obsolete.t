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
  $ cat >> commands.txt <<EOF
  > pick 177f92b77385 2 c
  > drop d2ae7f538514 1 b
  > pick 055a42cdd887 3 d
  > fold e860deea161a 4 e
  > pick 652413bf663e 5 f
  > EOF
  $ hg histedit 1 --commands commands.txt --verbose | grep histedit
  saved backup bundle to $TESTTMP/base/.hg/strip-backup/34a9919932c1-backup.hg (glob)
  $ hg log --graph --hidden
  @  8:0efacef7cb48 f
  |
  o  7:ae467701c500 d
  |
  o  6:d36c0562f908 c
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
  e860deea161a2f77de56603b340ebbb4536308ae ae467701c5006bf21ffcfdb555b3d6b63280b6b7 0 {'date': '*', 'user': 'test'} (glob)
  652413bf663ef2a641cab26574e46d5f5a64a55a 0efacef7cb481bf574f69075b82d044fdbe5c20f 0 {'date': '*': 'test'} (glob)
  d2ae7f538514cd87c17547b0de4cea71fe1af9fb 0 {'date': '*', 'user': 'test'} (glob)
  055a42cdd88768532f9cf79daa407fc8d138de9b ae467701c5006bf21ffcfdb555b3d6b63280b6b7 0 {'date': '*': 'test'} (glob)
  177f92b773850b59254aa5e923436f921b55483b d36c0562f908c692f5204d606d4ff3537d41f1bf 0 {'date': '*', 'user': 'test'} (glob)

Ensure hidden revision does not prevent histedit
-------------------------------------------------

create an hidden revision

  $ cat > commands.txt <<EOF
  > pick d36c0562f908 6 c
  > drop ae467701c500 7 d
  > pick 0efacef7cb48 8 f
  > EOF
  $ hg histedit 6 --commands commands.txt
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log --graph
  @  9:7c044e3e33a9 f
  |
  o  6:d36c0562f908 c
  |
  o  0:cb9a9f314b8b a
  
check hidden revision are ignored (6 have hidden children 7 and 8)

  $ cat > commands.txt <<EOF
  > pick d36c0562f908 6 c
  > pick 7c044e3e33a9 8 f
  > EOF
  $ hg histedit 6 --commands commands.txt
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved


Check that histedit respect phases
=========================================

(not directly related to the test file but doesn't deserve it's own test case)

  $ hg log -G
  @  9:7c044e3e33a9 f
  |
  o  6:d36c0562f908 c
  |
  o  0:cb9a9f314b8b a
  
  $ hg ph -pv '.^'
  phase changed for 2 changesets
  $ hg histedit -r '.~2'
  abort: cannot edit immutable changeset: cb9a9f314b8b
  [255]
