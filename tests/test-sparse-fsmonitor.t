This test doesn't yet work due to the way fsmonitor is integrated with test runner

  $ exit 80

test sparse interaction with other extensions

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > strip=
  > EOF

Test fsmonitor integration (if available)
TODO: make fully isolated integration test a'la https://github.com/facebook/watchman/blob/master/tests/integration/WatchmanInstance.py
(this one is using the systemwide watchman instance)

  $ touch .watchmanconfig
  $ echo "ignoredir1/" >> .hgignore
  $ hg commit -Am ignoredir1
  adding .hgignore
  $ echo "ignoredir2/" >> .hgignore
  $ hg commit -m ignoredir2

  $ hg sparse --reset
  $ hg sparse -I ignoredir1 -I ignoredir2 -I dir1

  $ mkdir ignoredir1 ignoredir2 dir1
  $ touch ignoredir1/file ignoredir2/file dir1/file

Run status twice to compensate for a condition in fsmonitor where it will check
ignored files the second time it runs, regardless of previous state (ask @sid0)
  $ hg status --config extensions.fsmonitor=
  ? dir1/file
  $ hg status --config extensions.fsmonitor=
  ? dir1/file

Test that fsmonitor ignore hash check updates when .hgignore changes

  $ hg up -q ".^"
  $ hg status --config extensions.fsmonitor=
  ? dir1/file
  ? ignoredir2/file
