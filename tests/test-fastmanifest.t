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
  >     python "$TESTTMP/summary.py" "$TESTTMP/cachedrevs" "$TESTTMP/logfile"
  >     rm "$TESTTMP/logfile"
  > }

  $ savecachedrevs() {
  >      (printf "%d " "-1"
  >       hg log -r "fastmanifesttocache()" -T "{rev} "
  >       echo "") > $TESTTMP/cachedrevs
  > }


  $ cat > $TESTTMP/summary.py << EOM
  > import sys
  > def summary(cached,accessed):
  >     accessed = open(accessed).readlines()[0]
  >     cached = open(cached).readlines()[0]
  >     accessedset = set(accessed.strip().split(' '))
  >     cachedset = set(cached.strip().split(' '))
  >     print '================================================='
  >     print 'CACHE MISS %s' % sorted(accessedset - cachedset)
  >     print 'CACHE HIT %s' % sorted(accessedset & cachedset)
  >     print '================================================='
  > summary(sys.argv[1], sys.argv[2])
  > EOM

  $ clearlogs() {
  >   rm "$TESTTMP/logfile"
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

  $ savecachedrevs
  $ mkcommit a

  $ savecachedrevs
  $ mkcommit b
  $ printaccessedrevs
  =================================================
  CACHE MISS []
  CACHE HIT ['-1']
  =================================================

  $ echo "c" > a
  $ savecachedrevs
  $ hg commit -m "new a"
  $ printaccessedrevs
  =================================================
  CACHE MISS []
  CACHE HIT ['1']
  =================================================

2) Diff

  $ savecachedrevs
  $ hg diff -c . > /dev/null
  $ printaccessedrevs
  =================================================
  CACHE MISS []
  CACHE HIT ['1']
  =================================================

  $ savecachedrevs
  $ hg diff -c ".^" > /dev/null
  $ printaccessedrevs
  =================================================
  CACHE MISS []
  CACHE HIT ['0']
  =================================================

  $ savecachedrevs
  $ hg diff -r ".^" > /dev/null
  $ clearlogs

3) Log (TODO)

4) Update

  $ savecachedrevs
  $ hg update ".^^" -q
  $ printaccessedrevs
  =================================================
  CACHE MISS []
  CACHE HIT ['0']
  =================================================

  $ savecachedrevs
  $ hg update tip -q
  $ printaccessedrevs
  =================================================
  CACHE MISS []
  CACHE HIT ['0']
  =================================================

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
  =================================================
  CACHE MISS []
  CACHE HIT ['2']
  =================================================
  $ hg rebase -r 5:: -d 4 --config extensions.rebase=
  rebasing 5:5234b99c4f1d "add e"
  rebasing 6:dd82c74514cb "add f" (tip)
  saved backup bundle to $TESTTMP/diagnosis/.hg/strip-backup/5234b99c4f1d-c2e049ad-backup.hg (glob)
  $ printaccessedrevs
  =================================================
  CACHE MISS ['6']
  CACHE HIT []
  =================================================
