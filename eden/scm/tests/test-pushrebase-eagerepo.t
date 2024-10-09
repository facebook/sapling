#modern-config-incompatible
#require no-eden
#inprocess-hg-incompatible

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ configure mutation dummyssh
  $ setconfig push.edenapi=true

Set up server repository

  $ newserver server
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ echo foo > a
  $ echo foo > b
  $ hg commit -Am 'initial'
  adding a
  adding b
  $ hg book master
  $ cd ..

Set up client repository

  $ hg clone --config 'extensions.remotenames=' ssh://user@dummy/server client -q

Test that pushing to a remotename preserves commit hash if no rebase happens

  $ cd client
  $ setconfig extensions.remotenames= extensions.pushrebase=
  $ hg up -q master
  $ echo x >> a && hg commit -qm 'add a'
  $ hg commit --amend -qm 'changed message'
  $ hg log -r . -T '{node}\n'
  ea98a8f9539083f60b81315106c94227e8814d17
  $ hg push --to master 2>&1 | grep error
  error.HttpError: not supported by the server
  $ hg log -r . -T '{node}\n'
  ea98a8f9539083f60b81315106c94227e8814d17
  $ hg log -G -r 'all()' -T '{node|short} {desc} {remotebookmarks} {bookmarks}'
  @  ea98a8f95390 changed message
  │
  o  2bb9d20e471c initial default/master
