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

  $ hgcloneshallow ssh://user@dummy/master shallow -q
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
  rebasing 2:0632994590a8 "xx" (tip)
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/0632994590a8-0bc786d8-backup.hg (glob)
  $ hg log -f x --template "{node|short}\n"
  81deab2073bc
  b292c1e3311f


# Rebase back, log -f still works

  $ hg rebase -d 0 -r 2
  rebasing 2:81deab2073bc "xx" (tip)
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/81deab2073bc-80cb4fda-backup.hg (glob)
  $ hg log -f x --template "{node|short}\n"
  b3fca10fb42d
  b292c1e3311f

  $ hg rebase -d 1 -r 2
  rebasing 2:b3fca10fb42d "xx" (tip)
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/b3fca10fb42d-da73a0c7-backup.hg (glob)

  $ cd ..

# Reset repos
  $ clearcache

  $ rm -rf master
  $ rm -rf shallow
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

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

# Rebase stack onto landed commit

  $ cd master
  $ echo x >> x
  $ hg commit -Aqm xx

  $ cd ../shallow
  $ echo x >> x
  $ hg commit -Aqm xx2
  $ echo y >> x
  $ hg commit -Aqm xxy

  $ hg pull -q
  $ hg rebase -d tip
  rebasing 1:4549721d828f "xx2"
  note: rebase of 1:4549721d828f created no changes to commit
  rebasing 2:5ef6d97e851c "xxy"
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/4549721d828f-b084e33c-backup.hg (glob)
  $ hg log -f x --template '{node|short}\n'
  4ae8e31c85ef
  0632994590a8
  b292c1e3311f

  $ cd ..

# system cache has invalid linknode, but .hg/store/data has valid

  $ cd shallow
  $ hg strip -r 1 -q
  $ rm -rf .hg/store/data/*
  $ echo x >> x
  $ hg commit -Aqm xx_local
  $ hg log -f x --template '{rev}:{node|short}\n'
  1:21847713771d
  0:b292c1e3311f

  $ cd ..
