#debugruntest-compatible

  $ enable amend
  $ setconfig extensions.smergebench=$TESTDIR/../contrib/smerge_benchmark.py

prepare a repo

  $ newrepo

test merge adjacent changes (tofix)

  $ cat > base <<EOF
  > a
  > b
  > c
  > d
  > e
  > EOF
  $ cat > src <<EOF
  > a
  > b'
  > c
  > d
  > e
  > EOF
  $ cat > dest <<EOF
  > a
  > b
  > c'
  > d'
  > e
  > EOF

  $ hg debugsmerge dest src base
  a
  <<<<<<< dest
   b
  -c
  -d
  +c'
  +d'
  =======
  -b
  +b'
   c
   d
  >>>>>>> source
  e
