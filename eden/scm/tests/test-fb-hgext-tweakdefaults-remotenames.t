#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible

  $ configure mutation-norecord modernclient

Set up
  $ enable rebase remotenames tweakdefaults

  $ newclientrepo repo
  $ echo a > a
  $ hg commit -qAm aa
  $ hg bookmark one -i
  $ hg push -q -r . --to one --create
  $ echo b > b
  $ hg commit -qAm bb
  $ hg bookmark two -i
  $ hg push -q -r . --to two --create
  $ echo c > c
  $ hg commit -qAm cc
  $ hg bookmark three -i
  $ hg push -q -r . --to three --create
  $ newclientrepo clone test:repo_server one two three

Test that hg pull --rebase aborts without --dest
  $ hg log -G --all -T '{node|short} {bookmarks} {remotenames}'
  @  083f922fc4a9  remote/three
  │
  o  301d76bdc3ae  remote/two
  │
  o  8f0162e483d0  remote/one
  
  $ hg up -q remote/one
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
  $ hg book bm -t remote/two
  $ hg pull --rebase
  pulling from test:repo_server (glob)
  rebasing 3de6bbccf693 "foo" (bm)
  $ hg pull --rebase --dest three
  pulling from test:repo_server (glob)
  rebasing 54ac787ff1c5 "foo" (bm)

Test that hg pull --update aborts without --dest
  $ hg pull --update
  abort: you must specify a destination for the update
  (use `hg pull --update --dest <destination>`)
  [255]
  $ hg pull --update --dest one
  pulling from test:repo_server (glob)
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  (leaving bookmark bm)

Test that setting a defaultdest allows --update and --rebase to work
  $ hg pull --update --config tweakdefaults.defaultdest=two
  pulling from test:repo_server (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G --all -T '{node|short} {bookmarks} {remotenames}'
  o  5413b62180b7 bm
  │
  o  083f922fc4a9  remote/three
  │
  @  301d76bdc3ae  remote/two
  │
  o  8f0162e483d0  remote/one
  
  $ echo d > d
  $ hg commit -qAm d
  $ hg pull --rebase --config tweakdefaults.defaultdest=three
  pulling from test:repo_server (glob)
  rebasing 50f3f60b4841 "d"
  $ hg log -G --all -T '{node|short} {bookmarks} {remotenames}'
  @  ba0f83735c95
  │
  │ o  5413b62180b7 bm
  ├─╯
  o  083f922fc4a9  remote/three
  │
  o  301d76bdc3ae  remote/two
  │
  o  8f0162e483d0  remote/one
  
Test that hg pull --rebase also works with a --tool argument
  $ echo d created at remote > ../repo/d
  $ hg -R ../repo update three -q
  $ hg -R ../repo commit -qAm 'remote d'
  $ hg -R ../repo push -r . -q --to three --create
  $ hg pull --rebase --dest three --tool internal:union
  pulling from test:repo_server (glob)
  searching for changes
  rebasing ba0f83735c95 "d"
  merging d
  $ hg log -G --all -T '{node|short} {bookmarks} {remotenames}'
  @  d6553cf01770
  │
  o  e8aa3bc9f3f0  remote/three
  │
  │ o  5413b62180b7 bm
  ├─╯
  o  083f922fc4a9
  │
  o  301d76bdc3ae  remote/two
  │
  o  8f0162e483d0  remote/one
  
  $ cat d
  d created at remote
  d
