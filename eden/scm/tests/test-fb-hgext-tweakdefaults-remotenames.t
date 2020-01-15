#chg-compatible

  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
Check for remotenames and skip if not present

Set up
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rebase=
  > remotenames=
  > tweakdefaults=
  > EOF

  $ hg init repo
  $ echo a > repo/a
  $ hg -R repo commit -qAm aa
  $ hg -R repo bookmark one -i
  $ echo b > repo/b
  $ hg -R repo commit -qAm bb
  $ hg -R repo bookmark two -i
  $ echo c > repo/c
  $ hg -R repo commit -qAm cc
  $ hg -R repo bookmark three -i
  $ hg clone -q repo clone
  $ cd clone

Test that hg pull --rebase aborts without --dest
  $ hg log -G --all -T '{node|short} {bookmarks} {remotenames}'
  @  083f922fc4a9  default/three
  |
  o  301d76bdc3ae  default/two
  |
  o  8f0162e483d0  default/one
  
  $ hg up -q default/one
  $ touch foo
  $ hg commit -qAm 'foo'
  $ hg pull --rebase
  abort: you must use a bookmark with tracking or manually specify a destination for the rebase
  (set up tracking with `hg book <name> -t <destination>` or manually supply --dest / -d)
  [255]
  $ hg bookmark bm
  $ hg pull --rebase
  abort: you must use a bookmark with tracking or manually specify a destination for the rebase
  (set up tracking with `hg book -t <destination>` or manually supply --dest / -d)
  [255]
  $ hg book bm -t default/two
  $ hg pull --rebase
  pulling from $TESTTMP/repo (glob)
  searching for changes
  no changes found
  rebasing 3de6bbccf693 "foo" (bm)
  saved backup bundle to $TESTTMP/clone/.hg/strip-backup/3de6bbccf693-0dce0663-rebase.hg (glob)
  $ hg pull --rebase --dest three
  pulling from $TESTTMP/repo (glob)
  searching for changes
  no changes found
  rebasing 54ac787ff1c5 "foo" (bm)
  saved backup bundle to $TESTTMP/clone/.hg/strip-backup/54ac787ff1c5-4c2ca3a1-rebase.hg (glob)

Test that hg pull --update aborts without --dest
  $ hg pull --update
  abort: you must specify a destination for the update
  (use `hg pull --update --dest <destination>`)
  [255]
  $ hg pull --update --dest one
  pulling from $TESTTMP/repo (glob)
  searching for changes
  no changes found
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  (leaving bookmark bm)

Test that setting a defaultdest allows --update and --rebase to work
  $ hg pull --update --config tweakdefaults.defaultdest=two
  pulling from $TESTTMP/repo (glob)
  searching for changes
  no changes found
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G --all -T '{node|short} {bookmarks} {remotenames}'
  o  5413b62180b7 bm
  |
  o  083f922fc4a9  default/three
  |
  @  301d76bdc3ae  default/two
  |
  o  8f0162e483d0  default/one
  
  $ echo d > d
  $ hg commit -qAm d
  $ hg pull --rebase --config tweakdefaults.defaultdest=three
  pulling from $TESTTMP/repo (glob)
  searching for changes
  no changes found
  rebasing 50f3f60b4841 "d"
  saved backup bundle to * (glob)
  $ hg log -G --all -T '{node|short} {bookmarks} {remotenames}'
  @  ba0f83735c95
  |
  | o  5413b62180b7 bm
  |/
  o  083f922fc4a9  default/three
  |
  o  301d76bdc3ae  default/two
  |
  o  8f0162e483d0  default/one
  
Test that hg pull --rebase also works with a --tool argument
  $ echo d created at remote > ../repo/d
  $ hg -R ../repo update three -q
  $ hg -R ../repo commit -qAm 'remote d'
  $ hg pull --rebase --dest three --tool internal:union
  pulling from $TESTTMP/repo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  rebasing ba0f83735c95 "d"
  merging d
  saved backup bundle to $TESTTMP/clone/.hg/strip-backup/ba0f83735c95-ba455273-rebase.hg (glob)
  $ hg log -G --all -T '{node|short} {bookmarks} {remotenames}'
  @  d6553cf01770
  |
  o  e8aa3bc9f3f0  default/three
  |
  | o  5413b62180b7 bm
  |/
  o  083f922fc4a9
  |
  o  301d76bdc3ae  default/two
  |
  o  8f0162e483d0  default/one
  
  $ cat d
  d created at remote
  d
