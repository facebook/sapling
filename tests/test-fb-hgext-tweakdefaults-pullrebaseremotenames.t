TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=

Set up with remotenames
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rebase=
  > remotenames=
  > tweakdefaults=
  > EOF

  $ hg init repo
  $ echo a > repo/a
  $ hg -R repo commit -qAm a
  $ hg clone -q repo clone
  $ cd clone

Pull --rebase with no local changes
  $ hg bookmark localbookmark -t default/default
  $ echo b > ../repo/b
  $ hg -R ../repo commit -qAm b
  $ hg pull --rebase
  pulling from $TESTTMP/repo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets d2ae7f538514
  (run 'hg update' to get a working copy)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  nothing to rebase - fast-forwarded to default/default
  $ hg log -G -T "{rev} {desc}: {bookmarks}"
  @  1 b: localbookmark
  |
  o  0 a:
  
Make a local commit and check pull --rebase still works.
  $ echo x > x
  $ hg commit -qAm x
  $ echo c > ../repo/c
  $ hg -R ../repo commit -qAm c
  $ hg pull --rebase
  pulling from $TESTTMP/repo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 177f92b77385
  (run 'hg heads' to see heads, 'hg merge' to merge)
  rebasing 2:* "x" (localbookmark) (glob)
  saved backup bundle * (glob)
  $ hg log -G -T "{rev} {desc}: {bookmarks}"
  @  3 x: localbookmark
  |
  o  2 c:
  |
  o  1 b:
  |
  o  0 a:
  
