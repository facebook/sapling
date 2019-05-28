  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
Set up without remotenames
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rebase=
  > tweakdefaults=
  > EOF

  $ hg init repo
  $ echo a > repo/a
  $ hg -R repo commit -qAm a
  $ hg clone -q repo clone
  $ cd clone

Pull --rebase with no local changes
  $ echo b > ../repo/b
  $ hg -R ../repo commit -qAm b
  $ hg pull --rebase -d default
  pulling from $TESTTMP/repo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets d2ae7f538514
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  nothing to rebase - fast-forwarded to default
  $ hg log -G -T "{rev} {desc}"
  @  1 b
  |
  o  0 a
  
Make a local commit and check pull --rebase still works.
  $ echo x > x
  $ hg commit -qAm x
  $ echo c > ../repo/c
  $ hg -R ../repo commit -qAm c
  $ hg pull --rebase -d default
  pulling from $TESTTMP/repo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 177f92b77385
  rebasing 2:* "x" (glob)
  saved backup bundle * (glob)
  $ hg log -G -T "{rev} {desc}"
  @  3 x
  |
  o  2 c
  |
  o  1 b
  |
  o  0 a
  
