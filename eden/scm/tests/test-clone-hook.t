#debugruntest-compatible

  $ configure modern
  $ newremoterepo repo1
  $ setconfig paths.default=test:e1

Show post-clone runs from within the new repo.
  $ hg clone -Uq test:e1 repo --config 'hooks.post-clone.foo=touch bar'
  $ ls repo
  bar
