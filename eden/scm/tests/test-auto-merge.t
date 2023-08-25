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
