#debugruntest-compatible

#require no-eden


  $ configure modern
  $ newremoterepo repo1
  $ setconfig paths.default=test:e1

Show post-clone runs from within the new repo.
  $ hg clone -Uq test:e1 repo --config 'hooks.post-clone.foo=touch bar' --config clone.use-rust=false
  $ ls repo
  bar
