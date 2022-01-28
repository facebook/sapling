#chg-compatible
#require mononoke
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
  $ setconfig remotefilelog.useruststore=True extensions.amend= rebase.experimental.inmemory=True

  $ hg up master
  fetching tree '' 6b8f81b9651010925578ea56a4129930688cbf98
  1 trees fetched over 0.00s
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up .~1
  fetching tree '' c6668049fdd8d48b367e0979dda40548062c0fca
  1 trees fetched over 0.00s
  fetching tree 'dir' e2dfe4bdd453d10c1e71df6634ffd1a6ac0a3892
  1 trees fetched over 0.00s
  fetching tree 'dir/ydir' 8a87e5128a9877c501d5a20c32dbd2103a54afad
  1 trees fetched over 0.00s
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ findfilessorted dir
  dir/x
  dir/ydir/y

  $ hg up .~1
  fetching tree '' 287ee6e53d4fbc5fab2157eb0383fdff1c3277c8
  1 trees fetched over 0.00s
  fetching tree 'dir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s
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
