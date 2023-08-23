#debugruntest-compatible

  $ configure modern
  $ enable shelve

Use wrong formatted '._*' files to mimic the binary files created by MacOS:

  $ newrepo simple
  $ drawdag << 'EOS'
  > d
  > | c
  > | |
  > | b
  > |/
  > a
  > EOS
  $ hg goto $c
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo c > c.txt
  $ hg add c.txt
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo 'wrong format' >> .hg/shelved/._default.oshelve
  $ echo 'wrong format' >> .hg/shelved/._default.patch

  $ hg log -r 'shelved()' -T '{desc}'
  shelve changes to: c (no-eol)
