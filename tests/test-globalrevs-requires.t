  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!

Add common configuration for the client and server.

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > pushrebase=
  > EOF


Configure the server

  $ hg init --config extensions.hgsql= --config extensions.globalrevs= \
  > --config format.useglobalrevs=True master
  $ configureserver master masterrepo
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > globalrevs=
  > [pushrebase]
  > blocknonpushrebase = True
  > EOF

  $ hg initglobalrev 0 --i-know-what-i-am-doing
  $ cd ..


Populate the database with an initial commit

  $ initclient client

  $ cd client
  $ touch x && hg ci -qAm x

  $ hg push -q ssh://user@dummy/master --to master


Test that `globalrevs` extensions is a requirement

  $ cd ../master

  $ grep globalrevs .hg/requires
  globalrevs

  $ hg log -r tip --config extensions.globalrevs=!
  abort: repository requires features unknown to this Mercurial: globalrevs!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]

  $ hg log -r tip
  changeset:   0:dc9179e745c2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  
