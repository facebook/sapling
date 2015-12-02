  $ . "$TESTDIR/library.sh"

# Populate the db with an initial commit

  $ initclient client
  $ cat >> client/.hg/hgrc <<EOF
  > [experimental]
  > bundle2-exp=True
  > EOF
  $ cd client
  $ echo x > x
  $ hg commit -qAm x
  $ hg bookmark mybook
  $ cd ..

  $ initserver master masterrepo
  $ cat >> master/.hg/hgrc <<EOF
  > [experimental]
  > bundle2-exp=True
  > bundle2lazylocking=True
  > EOF
  $ cd master
  $ hg log
  $ hg pull -q ../client

  $ cd ..

# Verify bookmarks are not synced if hook returns false

  $ cp master/.hg/hgrc master/.hg/hgrc_good
  $ cat >> master/.hg/hgrc <<EOF
  > [hooks]
  > pretxnclose.abort=false
  > EOF
  $ cd client
  $ echo x >> x
  $ hg commit -qm x2
  $ hg push ssh://user@dummy/master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: transaction abort!
  remote: rollback completed
  remote: pretxnclose.abort hook exited with status 1
  abort: push failed on remote
  [255]
  $ mv ../master/.hg/hgrc_good ../master/.hg/hgrc
  $ hg -R ../master bookmarks
     mybook                    0:b292c1e3311f

  $ cd ..

Test lazily acquiring the lock during unbundle

  $ cat >> $TESTTMP/locktester.py <<EOF
  > import os
  > from mercurial import extensions, bundle2, util
  > def checklock(orig, repo, *args, **kwargs):
  >     if len(repo.heldlocks) > 0:
  >         raise util.Abort("Lock should not be taken")
  >     return orig(repo, *args, **kwargs)
  > def extsetup(ui):
  >    extensions.wrapfunction(bundle2, 'processbundle', checklock)
  > EOF

  $ initserver lazylock lazylock
  $ cat >> lazylock/.hg/hgrc <<EOF
  > [extensions]
  > locktester=$TESTTMP/locktester.py
  > EOF

  $ initclient lazylockclient
  $ cd lazylockclient
  $ touch a && hg ci -Aqm a

- Push with lazy locking off (hook fails)
  $ hg push ssh://user@dummy/lazylock
  pushing to ssh://user@dummy/lazylock
  searching for changes
  remote: Lock should not be taken
  abort: push failed on remote
  [255]

- Push with lazy locking on (hook passes)
  $ cat >> ../lazylock/.hg/hgrc <<EOF
  > [experimental]
  > bundle2lazylocking=True
  > EOF
  $ hg push ssh://user@dummy/lazylock
  pushing to ssh://user@dummy/lazylock
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
