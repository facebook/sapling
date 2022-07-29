#chg-compatible
  $ setconfig experimental.allowfilepeer=True


  $ . "$TESTDIR/library.sh"

  $ enable lfs treemanifest pushrebase
  $ hginit master --config extensions.treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py

  $ cd master
  $ setconfig remotefilelog.server=True treemanifest.server=True remotefilelog.shallowtrees=True
  $ setconfig extensions.treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  $ mkdir dir
  $ echo x > dir/x
  $ hg commit -qAm x1
  $ hg book master
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow
  streaming all changes
  0 files to transfer, 0 bytes of data
  transferred 0 bytes in 0.0 seconds (0 bytes/sec)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' 287ee6e53d4fbc5fab2157eb0383fdff1c3277c8
  1 trees fetched over 0.00s
  fetching tree 'dir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd shallow
  $ enable remotenames
  $ setconfig treemanifest.sendtrees=True
  $ echo >> dir/x
  $ hg commit -m "Modify dir/x"
  $ hg push --to master
  pushing rev 6b73ab2c9773 to destination ssh://user@dummy/master bookmark master
  searching for changes
  updating bookmark master
  remote: pushing 1 changeset:
  remote:     6b73ab2c9773  Modify dir/x
  $ hg --cwd ../master log -G -l 1 --stat
  o  commit:      6b73ab2c9773
  │  bookmark:    master
  ~  user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     Modify dir/x
  
      dir/x |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  
