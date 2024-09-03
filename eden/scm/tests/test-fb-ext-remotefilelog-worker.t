#chg-compatible
#require mononoke
#debugruntest-incompatible
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ newserver master

  $ clone master client
  $ cd client
  $ mkdir dir
  $ echo x > dir/x
  $ hg commit -qAm x
  $ mkdir dir/ydir
  $ echo y > dir/ydir/y
  $ hg commit -qAm y
  $ hg rm dir/x
  $ hg rm dir/ydir/y
  $ hg commit -qAm rm
  $ hg push -q -r tip --to master --create
  $ cd ..

Shallow clone

  $ clone master shallow --noupdate
  $ cd shallow
  $ setconfig extensions.amend= rebase.experimental.inmemory=True

  $ hg up master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up .~1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ findfilessorted dir
  dir/x
  dir/ydir/y

  $ hg up .~1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ findfilessorted dir
  dir/x
  $ hg up master
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ test -f dir
  [1]

  $ echo x > x
  $ hg commit -qAm x
  $ echo y > y
  $ hg commit -qAm y
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [02b5b1] x
  $ hg amend --date "1 1"
  rebasing 02663ae2e9f7 "y"
