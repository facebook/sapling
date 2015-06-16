test sparse interaction with other extensions

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$(dirname $TESTDIR)/sparse.py
  > strip=
  > [simplecache]
  > cachedir=$TESTTMP/hgsimplecache
  > EOF

Test integration with simplecache for profile reads

  $ $PYTHON -c 'import simplecache' || exit 80
  $ printf "[include]\nfoo\n" > .hgsparse
  $ hg add .hgsparse
  $ hg commit -qm 'Add profile'
  $ hg sparse --enable-profile .hgsparse
  $ hg status --debug --config extensions.simplecache=
  falling back for value sparseprofile:.hgsparse:52fe6c0958d7d08df53bdf7ee62a261abb7f599e:v1
  set value for key sparseprofile:.hgsparse:52fe6c0958d7d08df53bdf7ee62a261abb7f599e:v1 to local
  $ hg status --debug --config extensions.simplecache=
  got value for key sparseprofile:.hgsparse:52fe6c0958d7d08df53bdf7ee62a261abb7f599e:v1 from local

Test hgwatchman integration (if available)

  $ $PYTHON -c 'import hgwatchman' || exit 80
  $ echo "ignoredir1/" >> .hgignore
  $ hg commit -Am ignoredir1
  adding .hgignore
  $ echo "ignoredir2/" >> .hgignore
  $ hg commit -m ignoredir2

  $ hg sparse --reset
  $ hg sparse -I ignoredir1 -I ignoredir2 -I dir1

  $ mkdir ignoredir1 ignoredir2 dir1
  $ touch ignoredir1/file ignoredir2/file dir1/file

Run status twice to compensate for a condition in hgwatchman where it will check
ignored files the second time it runs, regardless of previous state (ask @sid0)
  $ hg status --config extensions.hgwatchman=
  ? dir1/file
  $ hg status --config extensions.hgwatchman=
  ? dir1/file

Test that hgwatchmans ignore hash check updates when .hgignore changes

  $ hg up -q .^
  $ hg status --config extensions.hgwatchman=
  ? dir1/file
  ? ignoredir2/file
