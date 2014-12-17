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
  > EOF
  $ cd master
  $ hg log
  $ hg pull -q ../client

  $ cd ..

# Verify bookmarks are not synced if hook returns false

  $ cat >> master/.hg/hgrc <<EOF
  > [hooks]
  > b2x-pretransactionclose.abort=false
  > EOF
  $ cd client
  $ echo x >> x
  $ hg commit -qm x2
  $ hg push ssh://user@dummy/master
  pushing to ssh://user@dummy/master
  searching for changes
  abort: b2x-pretransactionclose.abort hook exited with status 1
  remote: transaction abort!
  remote: rollback completed
  [255]
  $ hg -R ../master bookmarks
     mybook                    0:b292c1e3311f
