Test the 'effect-flags' feature

Global setup
============

  $ . $TESTDIR/testlib/obsmarker-common.sh
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > interactive = true
  > [phases]
  > publish=False
  > [extensions]
  > rebase =
  > [experimental]
  > evolution = all
  > evolution.effect-flags = 1
  > EOF

  $ hg init $TESTTMP/effect-flags
  $ cd $TESTTMP/effect-flags
  $ mkcommit ROOT

amend touching the description only
-----------------------------------

  $ mkcommit A0
  $ hg commit --amend -m "A1"

check result

  $ hg debugobsolete --rev .
  471f378eab4c5e25f6c77f785b27c936efb22874 fdf9bde5129a28d4548fadd3f62b265cdd3b7a2e 0 (Thu Jan 01 00:00:00 1970 +0000) {'ef1': '1', 'operation': 'amend', 'user': 'test'}

amend touching the user only
----------------------------

  $ mkcommit B0
  $ hg commit --amend -u "bob <bob@bob.com>"

check result

  $ hg debugobsolete --rev .
  ef4a313b1e0ade55718395d80e6b88c5ccd875eb 5485c92d34330dac9d7a63dc07e1e3373835b964 0 (Thu Jan 01 00:00:00 1970 +0000) {'ef1': '16', 'operation': 'amend', 'user': 'test'}

amend touching the date only
----------------------------

  $ mkcommit B1
  $ hg commit --amend -d "42 0"

check result

  $ hg debugobsolete --rev .
  2ef0680ff45038ac28c9f1ff3644341f54487280 4dd84345082e9e5291c2e6b3f335bbf8bf389378 0 (Thu Jan 01 00:00:00 1970 +0000) {'ef1': '32', 'operation': 'amend', 'user': 'test'}

amend touching the branch only
----------------------------

  $ mkcommit B2
  $ hg branch my-branch
  marked working directory as branch my-branch
  (branches are permanent and global, did you want a bookmark?)
  $ hg commit --amend

check result

  $ hg debugobsolete --rev .
  bd3db8264ceebf1966319f5df3be7aac6acd1a8e 14a01456e0574f0e0a0b15b2345486a6364a8d79 0 (Thu Jan 01 00:00:00 1970 +0000) {'ef1': '64', 'operation': 'amend', 'user': 'test'}

  $ hg up default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

rebase (parents change)
-----------------------

  $ mkcommit C0
  $ mkcommit D0
  $ hg rebase -r . -d 'desc(B0)'
  rebasing 10:c85eff83a034 "D0" (tip)

check result

  $ hg debugobsolete --rev .
  c85eff83a0340efd9da52b806a94c350222f3371 da86aa2f19a30d6686b15cae15c7b6c908ec9699 0 (Thu Jan 01 00:00:00 1970 +0000) {'ef1': '4', 'operation': 'rebase', 'user': 'test'}

amend touching the diff
-----------------------

  $ mkcommit E0
  $ echo 42 >> E0
  $ hg commit --amend

check result

  $ hg debugobsolete --rev .
  ebfe0333e0d96f68a917afd97c0a0af87f1c3b5f 75781fdbdbf58a987516b00c980bccda1e9ae588 0 (Thu Jan 01 00:00:00 1970 +0000) {'ef1': '8', 'operation': 'amend', 'user': 'test'}

amend with multiple effect (desc and meta)
-------------------------------------------

  $ mkcommit F0
  $ hg branch my-other-branch
  marked working directory as branch my-other-branch
  $ hg commit --amend -m F1 -u "bob <bob@bob.com>" -d "42 0"

check result

  $ hg debugobsolete --rev .
  fad47e5bd78e6aa4db1b5a0a1751bc12563655ff a94e0fd5f1c81d969381a76eb0d37ce499a44fae 0 (Thu Jan 01 00:00:00 1970 +0000) {'ef1': '113', 'operation': 'amend', 'user': 'test'}

rebase not touching the diff
----------------------------

  $ cat << EOF > H0
  > 0
  > 1
  > 2
  > 3
  > 4
  > 5
  > 6
  > 7
  > 8
  > 9
  > 10
  > EOF
  $ hg add H0
  $ hg commit -m 'H0'
  $ echo "H1" >> H0
  $ hg commit -m "H1"
  $ hg up -r "desc(H0)"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat << EOF > H0
  > H2
  > 0
  > 1
  > 2
  > 3
  > 4
  > 5
  > 6
  > 7
  > 8
  > 9
  > 10
  > EOF
  $ hg commit -m "H2"
  created new head
  $ hg rebase -s "desc(H1)" -d "desc(H2)" -t :merge3
  rebasing 17:b57fed8d8322 "H1"
  merging H0
  $ hg debugobsolete -r tip
  b57fed8d83228a8ae3748d8c3760a77638dd4f8c e509e2eb3df5d131ff7c02350bf2a9edd0c09478 0 (Thu Jan 01 00:00:00 1970 +0000) {'ef1': '4', 'operation': 'rebase', 'user': 'test'}

amend closing the branch should be detected as meta change
----------------------------------------------------------

  $ hg branch closedbranch
  marked working directory as branch closedbranch
  $ mkcommit G0
  $ mkcommit I0
  $ hg commit --amend --close-branch

check result

  $ hg debugobsolete -r .
  2f599e54c1c6974299065cdf54e1ad640bfb7b5d 12c6238b5e371eea00fd2013b12edce3f070928b 0 (Thu Jan 01 00:00:00 1970 +0000) {'ef1': '2', 'operation': 'amend', 'user': 'test'}
