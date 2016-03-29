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


  $ printandclearlog() {
  >     [ ! -f "$TESTTMP/logfile" ] && echo "no access" && return
  >     cat "$TESTTMP/logfile" | sort | uniq
  >     rm "$TESTTMP/logfile"
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

  $ mkcommit a
  $ printandclearlog
  -1

  $ mkcommit b
  $ printandclearlog
  -1
  0

  $ echo "c" > a
  $ hg commit -m "new a"
  $ printandclearlog
  -1
  1

2) Diff

  $ hg diff -c . > /dev/null
  $ printandclearlog
  1
  2

  $ hg diff -c ".^" > /dev/null
  $ printandclearlog
  0
  1

  $ hg diff -r ".^" > /dev/null
  $ printandclearlog
  1
  2
3) Log

  $ hg log a > /dev/null
  $ printandclearlog
  no access

4) Update

  $ hg update ".^^" -q
  $ printandclearlog
  0
  2

  $ hg update tip -q
  $ printandclearlog
  0
  2
