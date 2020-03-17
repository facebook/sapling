#chg-compatible

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
  $ setconfig remotefilelog.useruststore=True worker.rustworkers=True extensions.amend= rebase.experimental.inmemory=True
  $ hg up master
  fetching tree '' 6b8f81b9651010925578ea56a4129930688cbf98, found via baeb6587a441
  1 trees fetched over 0.00s
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up .~1
  fetching tree '' c6668049fdd8d48b367e0979dda40548062c0fca, based on 6b8f81b9651010925578ea56a4129930688cbf98, found via 700dd3ba6cb0
  3 trees fetched over 0.00s
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ findfilessorted dir
  dir/x
  dir/ydir/y
  $ hg up .~1
  fetching tree '' 287ee6e53d4fbc5fab2157eb0383fdff1c3277c8, based on c6668049fdd8d48b367e0979dda40548062c0fca
  2 trees fetched over 0.00s
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
  $ hg amend
  rebasing 02663ae2e9f7 "y"
  hint[amend-autorebase]: descendants have been auto-rebased because no merge conflict could have happened - use --no-rebase or set commands.amend.autorebase=False to disable auto rebase
  hint[hint-ack]: use 'hg hint --ack amend-autorebase' to silence these hints
