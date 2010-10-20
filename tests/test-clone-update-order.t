  $ hg init
  $ echo foo > bar
  $ hg commit -Am default
  adding bar
  $ hg up -r null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg branch mine
  marked working directory as branch mine
  $ echo hello > world
  $ hg commit -Am hello
  adding world
  $ hg up -r null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg branch other
  marked working directory as branch other
  $ echo good > bye
  $ hg commit -Am other
  adding bye
  $ hg up -r mine
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg clone -U -u . .#other ../b -r 0 -r 1 -r 2 -b other
  abort: cannot specify both --noupdate and --updaterev
  [255]

  $ hg clone -U .#other ../b -r 0 -r 1 -r 2 -b other
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+2 heads)
  $ rm -rf ../b

  $ hg clone -u . .#other ../b -r 0 -r 1 -r 2 -b other
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+2 heads)
  updating to branch mine
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf ../b

  $ hg clone -u 0 .#other ../b -r 0 -r 1 -r 2 -b other
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+2 heads)
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf ../b

  $ hg clone -u 1 .#other ../b -r 0 -r 1 -r 2 -b other
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+2 heads)
  updating to branch mine
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf ../b

  $ hg clone -u 2 .#other ../b -r 0 -r 1 -r 2 -b other
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+2 heads)
  updating to branch other
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf ../b

Test -r mine ... mine is ignored:

  $ hg clone -u 2 .#other ../b -r mine -r 0 -r 1 -r 2 -b other
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+2 heads)
  updating to branch other
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf ../b

  $ hg clone .#other ../b -b default -b mine
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+2 heads)
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf ../b

  $ hg clone .#other ../b
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch other
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf ../b

  $ hg clone -U . ../c -r 1 -r 2 > /dev/null
  $ hg clone ../c ../b
  updating to branch other
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf ../b ../c

