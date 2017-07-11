  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

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
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/0632994590a8-0bc786d8-rebase.hg (glob)
  $ hg log -f x --template "{node|short}\n"
  81deab2073bc
  b292c1e3311f


# Rebase back, log -f still works

  $ hg rebase -d 0 -r 2
  rebasing 2:81deab2073bc "xx" (tip)
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/81deab2073bc-80cb4fda-rebase.hg (glob)
  $ hg log -f x --template "{node|short}\n"
  b3fca10fb42d
  b292c1e3311f

  $ hg rebase -d 1 -r 2
  rebasing 2:b3fca10fb42d "xx" (tip)
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/b3fca10fb42d-da73a0c7-rebase.hg (glob)

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
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/4549721d828f-b084e33c-rebase.hg (glob)
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
  $ rm -rf shallow

/* Local linknode is invalid; remote linknode is valid (formerly slow case) */

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd shallow
  $ echo x >> x
  $ hg commit -Aqm xx2
  $ cd ../master
  $ echo y >> y
  $ hg commit -Aqm yy2
  $ echo x >> x
  $ hg commit -Aqm xx2-fake-rebased
  $ echo y >> y
  $ hg commit -Aqm yy3
  $ cd ../shallow
  $ hg pull --config remotefilelog.debug=True
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 0 changes to 0 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg update tip -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ echo x > x
  $ hg commit -qAm xx3

# At this point, the linknode points to c1254e70bad1 instead of 32e6611f6149
  $ hg log -G -T '{node|short} {desc} {phase} {files}\n'
  @  a5957b6bf0bd xx3 draft x
  |
  o  7200df4e0aca yy3 public y
  |
  o  32e6611f6149 xx2-fake-rebased public x
  |
  o  01979f9404f8 yy2 public y
  |
  | o  c1254e70bad1 xx2 draft x
  |/
  o  0632994590a8 xx public x
  |
  o  b292c1e3311f x public x
  
# Check the contents of the local blob for incorrect linknode
  $ hg debugremotefilelog .hg/store/data/11f6ad8ec52a2984abaafd7c3b516503785c2072/d4a3ed9310e5bd9887e3bf779da5077efab28216
  size: 6 bytes
  path: .hg/store/data/11f6ad8ec52a2984abaafd7c3b516503785c2072/d4a3ed9310e5bd9887e3bf779da5077efab28216 
  key: d4a3ed9310e5 
  
          node =>           p1            p2      linknode     copyfrom
  d4a3ed9310e5 => aee31534993a  000000000000  c1254e70bad1  
  aee31534993a => 1406e7411862  000000000000  0632994590a8  
  1406e7411862 => 000000000000  000000000000  b292c1e3311f  

# Verify that we do a fetch on the first log (remote blob fetch for linkrev fix)
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

# But not after that
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x

# Check the contents of the remote blob for correct linknode
  $ hg debugremotefilelog $CACHEDIR/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/d4a3ed9310e5bd9887e3bf779da5077efab28216
  size: 6 bytes
  path: $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/d4a3ed9310e5bd9887e3bf779da5077efab28216 
  key: d4a3ed9310e5 
  
          node =>           p1            p2      linknode     copyfrom
  d4a3ed9310e5 => aee31534993a  000000000000  32e6611f6149  
  aee31534993a => 1406e7411862  000000000000  0632994590a8  
  1406e7411862 => 000000000000  000000000000  b292c1e3311f  
