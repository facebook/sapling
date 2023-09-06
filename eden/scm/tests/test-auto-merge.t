#debugruntest-compatible

  $ enable amend
  $ setconfig extensions.smergebench=$TESTDIR/../contrib/smerge_benchmark.py

prepare a repo

  $ newrepo

test merge adjacent changes

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
  b'
  c'
  d'
  e

test merge adjacent changes -- insertion case

  $ cat > base <<EOF
  > a
  > b
  > e
  > EOF
  $ cat > src <<EOF
  > a
  > b'
  > e
  > EOF
  $ cat > dest <<EOF
  > a
  > a2
  > b
  > c
  > d
  > e
  > EOF

  $ hg debugsmerge dest src base
  a
  <<<<<<< dest
  +a2
   b
  +c
  +d
  =======
  -b
  +b'
  >>>>>>> source
  e

test common changes

  $ cat > base <<EOF
  > a
  > d
  > EOF
  $ cat > src <<EOF
  > a
  > b
  > d
  > EOF
  $ cat > dest <<EOF
  > a
  > b
  > c
  > d
  > EOF
  $ hg debugsmerge dest src base
  a
  b
  c
  d
