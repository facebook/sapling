#debugruntest-compatible

  $ configure modern
  $ newremoterepo repo1
  $ setconfig paths.default=test:e1

Show post-clone runs from within the new repo.
  $ hg clone -Uq test:e1 repo --config 'hooks.post-clone.foo=touch bar' --config clone.use-rust=false
  $ ls repo
  bar

Get a warning about unrun hook with new clone.
  $ hg clone -Uq test:e1 repo repo2 --config hooks.post-clone.foo=_ --config hooks.pre-clone.bar=_ --config hooks.fail-clone.baz=_ --config clone.use-rust=true
  WARNING: The following hooks were not run: ["pre-clone.bar", "post-clone.foo"]

Repo already exists - test fail hook warning.
  $ hg clone -Uq test:e1 repo repo2 --config hooks.post-clone.foo=_ --config hooks.pre-clone.bar=_ --config hooks.fail-clone.baz=_ --config clone.use-rust=true
  abort: .hg directory already exists at clone destination * (glob)
  WARNING: The following hooks were not run: ["pre-clone.bar", "fail-clone.baz"]
  [255]

