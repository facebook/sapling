#debugruntest-compatible
#chg-compatible

  $ configure modern
  $ enable shelve

Use wrong format ._* files to mimic the binary files created by MacOS:

  $ newrepo simple
  $ drawdag << 'EOS'
  > d
  > | c
  > | |
  > | b
  > |/
  > a
  > EOS
  $ hg bookmark -r $d master
  $ hg goto $c
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo c > c.txt
  $ hg add c.txt
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo 'wrong format' >> .hg/shelved/._default.oshelve
  $ echo 'wrong format' >> .hg/shelved/._default.patch

  $ hg log -r 'shelved()' 2>&1 | head -n 1
  ** * has crashed: (glob)
