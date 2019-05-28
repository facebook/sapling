  $ setconfig extensions.treemanifest=!
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > reset=
  > remotenames=
  > EOF

  $ hg init repo
  $ cd repo

  $ echo x > x
  $ hg commit -qAm x
  $ hg book foo
  $ echo x >> x
  $ hg commit -qAm x2

Resetting past a remote bookmark should not delete the remote bookmark

  $ cd ..
  $ hg clone repo client
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client
  $ hg book bar
  $ hg reset --clean "default/foo^"
  $ hg log -G -T '{node|short} {bookmarks} {remotebookmarks}\n'
  o  a89d614e2364  default/foo
  |
  @  b292c1e3311f bar
  
