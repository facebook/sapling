Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

Check diagnosis, debugging information
1) Setup configuration
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    hg ci -l msg
  > }


  $ printaccessedrevs() {
  >     [ ! -f "$TESTTMP/logfile" ] && echo "no access" && return
  >     cat "$TESTTMP/logfile" | sort | uniq
  >     rm "$TESTTMP/logfile"
  > }

  $ printcachedrevs() {
  > hg log -r "fastmanifesttocache()" -T "{rev}\n"
  > }

  $ mkdir diagnosis
  $ cd diagnosis
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > fastmanifest=
  > [fastmanifest]
  > logfile=$TESTTMP/logfile
  > EOF


1) Commit

  $ printcachedrevs
  $ mkcommit a
  $ printaccessedrevs
  -1

  $ printcachedrevs
  0
  $ mkcommit b
  $ printaccessedrevs
  -1
  0

  $ echo "c" > a
  $ printcachedrevs
  0
  1
  $ hg commit -m "new a"
  $ printaccessedrevs
  -1
  1

2) Diff

  $ printcachedrevs
  0
  1
  2
  $ hg diff -c . > /dev/null
  $ printaccessedrevs
  1
  2

  $ printcachedrevs
  0
  1
  2
  $ hg diff -c ".^" > /dev/null
  $ printaccessedrevs
  0
  1

  $ printcachedrevs
  0
  1
  2
  $ hg diff -r ".^" > /dev/null
  $ printaccessedrevs
  1
  2

3) Log

  $ printcachedrevs
  0
  1
  2
  $ hg log a > /dev/null
  $ printaccessedrevs
  no access

4) Update

  $ printcachedrevs
  0
  1
  2
  $ hg update ".^^" -q
  $ printaccessedrevs
  0
  2

  $ printcachedrevs
  0
  1
  2
  $ hg update tip -q
  $ printaccessedrevs
  0
  2

5) Rebase
  $ mkcommit c
  $ mkcommit d
  $ hg update ".^^" -q
  $ mkcommit e
  created new head
  $ mkcommit f
  $ hg log -G -r 0:: -T "{rev} {node} {desc|firstline}"
  @  6 dd82c74514cbce45a3c61caf7ffaba16de19cec4 add f
  |
  o  5 5234b99c4f1d5b2ea45ea608550c66015f8f37ac add e
  |
  | o  4 cab0f51bb3f5493da8e7406e3967ef925e2e7a1f add d
  | |
  | o  3 329ad08f9742620b0b3be4305ca0c911d5517e84 add c
  |/
  o  2 00e42334abdae99958cd58b9be90fc940ca2b491 new a
  |
  o  1 7c3bad9141dcb46ff89abf5f61856facd56e476c add b
  |
  o  0 1f0dee641bb7258c56bd60e93edfa2405381c41e add a
  

  $ printaccessedrevs
  -1
  2
  3
  4
  5
  $ hg rebase -r 5:: -d 4 --config extensions.rebase=
  rebasing 5:5234b99c4f1d "add e"
  rebasing 6:dd82c74514cb "add f" (tip)
  saved backup bundle to $TESTTMP/diagnosis/.hg/strip-backup/5234b99c4f1d-c2e049ad-backup.hg (glob)
  $ printaccessedrevs
  -1
  2
  4
  5
  6
  7
  8

