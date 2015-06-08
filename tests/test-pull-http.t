#require killdaemons

  $ hg init test
  $ cd test
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ cd ..
  $ hg clone test test2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd test2
  $ echo a >> a
  $ hg ci -mb

Cloning with a password in the URL should not save the password in .hg/hgrc:

  $ hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg clone http://foo:xyzzy@localhost:$HGPORT/ test3
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat test3/.hg/hgrc
  # example repository config (see "hg help config" for more info)
  [paths]
  default = http://foo@localhost:$HGPORT/
  
  # path aliases to other clones of this repo in URLs or filesystem paths
  # (see "hg help config.paths" for more info)
  #
  # default-push = ssh://jdoe@example.net/hg/jdoes-fork
  # my-fork      = ssh://jdoe@example.net/hg/jdoes-fork
  # my-clone     = /home/jdoe/jdoes-clone
  
  [ui]
  # name and email (local to this repository, optional), e.g.
  # username = Jane Doe <jdoe@example.com>
  $ killdaemons.py

expect error, cloning not allowed

  $ echo '[web]' > .hg/hgrc
  $ echo 'allowpull = false' >> .hg/hgrc
  $ hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT/ test4 --config experimental.bundle2-exp=True
  requesting all changes
  abort: authorization failed
  [255]
  $ hg clone http://localhost:$HGPORT/ test4 --config experimental.bundle2-exp=False
  abort: authorization failed
  [255]
  $ killdaemons.py

serve errors

  $ cat errors.log
  $ req() {
  >     hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  >     cat hg.pid >> $DAEMON_PIDS
  >     hg --cwd ../test pull http://localhost:$HGPORT/
  >     killdaemons.py hg.pid
  >     echo % serve errors
  >     cat errors.log
  > }

expect error, pulling not allowed

  $ req
  pulling from http://localhost:$HGPORT/
  searching for changes
  abort: authorization failed
  % serve errors

  $ cd ..
