  $ setconfig extensions.treemanifest=!
Set up extension and repos

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > remotenames=
  > convert=
  > EOF

  $ hg init repo1
  $ cd repo1
  $ echo a > a
  $ hg add a
  $ hg commit -qm 'a'
  $ hg boo bm2
  $ cd ..
  $ hg clone repo1 repo2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test colors

  $ hg -R repo2 bookmark --remote
     default/bm2               0:cb9a9f314b8b
  $ hg convert repo2 repo3
  initializing destination repo3 repository
  scanning source...
  sorting...
  converting...
  0 a
  updating bookmarks
  $ hg -R repo3 bookmark
     default/bm2               0:cb9a9f314b8b
