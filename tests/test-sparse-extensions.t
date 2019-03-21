
test sparse interaction with other extensions

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > # Remove once default-on:
  > simplecache=
  > [simplecache]
  > showdebug=true
  > cachedir=$TESTTMP/hgsimplecache
  > EOF

Test integration with simplecache for profile reads

  $ printf "[include]\nfoo\n.gitignore\n" > .hgsparse
  $ hg add .hgsparse
  $ hg commit -qm 'Add profile'
  $ hg sparse --enable-profile .hgsparse
  $ hg status --debug
  got value for key sparseprofile:.hgsparse:090ca0df22bcfedb0d8c8cb8c66865529e714404:v2 from local
  got value for key sparseprofile:.hgsparse:090ca0df22bcfedb0d8c8cb8c66865529e714404:v2 from local

#if fsmonitor
Test fsmonitor integration (if available)

  $ touch .watchmanconfig
  $ echo "ignoredir1" >> .gitignore
  $ hg commit -Am ignoredir1
  adding .gitignore
  $ echo "ignoredir2" >> .gitignore
  $ hg commit -m ignoredir2

  $ hg sparse reset
  $ hg sparse -I ignoredir1 -I ignoredir2 -I dir1 -I .gitignore

  $ mkdir ignoredir1 ignoredir2 dir1
  $ touch ignoredir1/file ignoredir2/file dir1/file

Run status twice to compensate for a condition in fsmonitor where it will check
ignored files the second time it runs, regardless of previous state (ask @sid0)
  $ hg status
  ? dir1/file
  $ hg status
  ? dir1/file

Test that fsmonitor by default handles .gitignore changes and can "unignore" files.

  $ hg up -q ".^"
  $ hg status
  ? dir1/file
  ? ignoredir2/file

#endif
