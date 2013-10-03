# Tests for the complicated linknode logic in remotefilelog.py::ancestormap()

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > serverexpiration=-1
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ hgcloneshallow ssh://localhost/$PWD/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)


# Rebase produces correct log -f linknodes

  $ cd shallow
  $ echo y > y
  $ hg commit -qAm y
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo x >> x
  $ hg commit -qAm xx
  $ hg log -f x --template "{node|short}\n"
  0632994590a8
  b292c1e3311f

  $ hg rebase -d 1
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/0632994590a8-backup.hg
  $ hg log -f x --template "{node|short}\n"
  81deab2073bc
  b292c1e3311f


# Rebase back, log -f still works

  $ hg rebase -d 0 -r 2
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/81deab2073bc-backup.hg
  $ hg log -f x --template "{node|short}\n"
  b3fca10fb42d
  b292c1e3311f

  $ hg rebase -d 1 -r 2
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/b3fca10fb42d-backup.hg


# Deleting current file version is recoverable
# This seems to happen occasionally. Not sure how.
# For now, it produces a scary "remote: abort:" warning :(

  $ cp .hg/store/data/11f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d51 .hg/store/data/11f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d512
  $ rm .hg/store/data/11f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d51

  $ hg log -f x --template "{node|short}\n"
  remote: abort: data/x.i@aee31534993a: no match found!
  e9fd0afe47d0
  b292c1e3311f
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)


# Missing file fails

  $ rm .hg/store/data/11f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d51
  $ rm .hg/store/data/11f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d512

  $ hg log -f x
  remote: abort: data/x.i@aee31534993a: no match found!
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s
  abort: No such file or directory: '$TESTTMP/shallow/.hg/store/data/11f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d51'
  [255]
