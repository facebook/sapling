
test sparse interaction with other extensions

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$TESTDIR/../hgext/fbsparse.py
  > strip=
  > # Remove once default-on:
  > simplecache=
  > [simplecache]
  > showdebug=true
  > cachedir=$TESTTMP/hgsimplecache
  > EOF

Test integration with simplecache for profile reads

  $ $PYTHON -c 'import hgext.simplecache' || exit 80
  $ printf "[include]\nfoo\n" > .hgsparse
  $ hg add .hgsparse
  $ hg commit -qm 'Add profile'
  $ hg sparse --enable-profile .hgsparse
  $ hg status --debug
  got value for key sparseprofile:.hgsparse:52fe6c0958d7d08df53bdf7ee62a261abb7f599e:v1 from local

#if fsmonitor
Test fsmonitor integration (if available)
TODO: make fully isolated integration test a'la https://github.com/facebook/watchman/blob/master/tests/integration/WatchmanInstance.py
(this one is using the systemwide watchman instance)

  $ touch .watchmanconfig
  $ echo "ignoredir1/" >> .hgignore
  $ hg commit -Am ignoredir1
  adding .hgignore
  $ echo "ignoredir2/" >> .hgignore
  $ hg commit -m ignoredir2

  $ hg sparse reset
  $ hg sparse -I ignoredir1 -I ignoredir2 -I dir1

  $ mkdir ignoredir1 ignoredir2 dir1
  $ touch ignoredir1/file ignoredir2/file dir1/file

Run status twice to compensate for a condition in fsmonitor where it will check
ignored files the second time it runs, regardless of previous state (ask @sid0)
  $ hg status
  ? dir1/file
  $ hg status
  ? dir1/file

Test that fsmonitor ignore hash check updates when .hgignore changes

  $ hg up -q ".^"
  $ hg status
  ? dir1/file

BUG: treestate ignores ignore hash. So "? ignoredir2/file" did not show up.
#endif
