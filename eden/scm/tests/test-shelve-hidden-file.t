
  $ enable shelve

Use wrong formatted '._*' files to mimic the binary files created by MacOS:

  $ newclientrepo simple << 'EOS'
  > d
  > | c
  > | |
  > | b
  > |/
  > a
  > EOS
#if no-eden
TODO(sggutier): This should work on EdenFS, but there seems to be a bug in EagerRepo's implementation
  $ hg goto $c
  pulling 'a82ac2b3875752239b995aabd5b4e9712db0bc9e' from 'test:simple_server'
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
#endif
