#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ enable remotenames
  $ enable amend

Set up repositories

  $ hg init repo1
  $ hg clone -q repo1 repo2
  $ hg clone -q repo1 repo3

Set up the repos with a remote bookmark

  $ cd repo2
  $ echo a > a
  $ hg commit -Aqm commitA
  $ hg push -q --to book --create
  $ cd ..

  $ cd repo3
  $ hg pull -q -B book
  $ cd ..

Produce a new commit in repo2

  $ cd repo2
  $ echo b > b
  $ hg commit -Aqm commitB
  $ hg bundle -q -r . ../bundleB
  $ hg push -q --to book
  $ cd ..

Load the commit in repo3, hide it, check that we can still pull.

  $ cd repo3

  $ hg unbundle -q ../bundleB
  $ hg log -r tip -T '{desc}\n'
  commitB
  $ hg hide -q -r tip

  $ hg goto -q default/book
  $ hg log -r tip -T '{desc}\n'
  commitB

  $ hg pull -q
  $ hg log -r "reverse(::book)" -T '{desc}\n'
  commitB
  commitA

  $ cd ..
