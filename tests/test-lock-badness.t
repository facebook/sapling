#require unix-permissions no-root no-windows

Prepare

  $ hg init a
  $ echo a > a/a
  $ hg -R a ci -A -m a
  adding a

  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

One process waiting for another

  $ cat > hooks.py << EOF
  > import time
  > def sleepone(**x): time.sleep(1)
  > def sleephalf(**x): time.sleep(0.5)
  > EOF
  $ echo b > b/b
  $ hg -R b ci -A -m b --config hooks.precommit="python:`pwd`/hooks.py:sleepone" > stdout &
  $ hg -R b up -q --config hooks.pre-update="python:`pwd`/hooks.py:sleephalf"
  waiting for lock on working directory of b held by '*:*' (glob)
  got lock after ? seconds (glob)
  warning: ignoring unknown working parent d2ae7f538514!
  $ wait
  $ cat stdout
  adding b

Pushing to a local read-only repo that can't be locked

  $ chmod 100 a/.hg/store

  $ hg -R b push a
  pushing to a
  searching for changes
  abort: could not lock repository a: Permission denied
  [255]

  $ chmod 700 a/.hg/store
