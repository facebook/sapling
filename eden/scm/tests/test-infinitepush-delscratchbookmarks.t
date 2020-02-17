#chg-compatible

  $ configure dummyssh
  $ disable treemanifest
  $ enable infinitepush remotenames
  $ setconfig infinitepush.branchpattern="re:scratch/.+"
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }

Create server repo
  $ hg init repo
  $ cd repo
  $ mkcommit servercommit
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server=yes
  > indextype=disk
  > storetype=disk
  > EOF
  $ cd ..

Create second server repo
  $ hg init repo2
  $ cd repo2
  $ mkcommit servercommit2
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server=yes
  > indextype=disk
  > storetype=disk
  > EOF
  $ cd ..

Clone server
  $ hg clone ssh://user@dummy/repo --config extensions.remotenames= client -q
  $ cd client

Ensure no bookmarks
  $ hg book --remote
  $ hg book
  no bookmarks set

Push scratch bookmark
  $ mkcommit scratchcommit1
  $ hg push default -r . --to scratch/test1 --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     45f7b362ad7c  scratchcommit1
  $ hg book --remote
     default/scratch/test1     1:45f7b362ad7c

Delete scratch bookmark
  $ hg book -d scratch/test1
  $ hg book --remote

Check regular deletion still works
  $ hg book testlocal1
  $ hg book
   * testlocal1                1:45f7b362ad7c
  $ hg book -d testlocal1
  $ hg book
  no bookmarks set

Test deleting both regular and scratch
  $ hg push default -r . --to scratch/test2 --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     45f7b362ad7c  scratchcommit1
  $ hg book testlocal2
  $ hg book -a
   * testlocal2                1:45f7b362ad7c
     default/scratch/test2     1:45f7b362ad7c
  $ hg book -d testlocal2 scratch/test2
  $ hg book -a
  no bookmarks set

Test deleting nonexistent bookmarks
  $ hg book -d scratch/nonexistent1
  abort: infinitepush bookmark 'scratch/nonexistent1' does not exist in path 'default'
  [255]
  $ hg book -d localnonexistent1
  abort: bookmark 'localnonexistent1' does not exist
  [255]
  $ hg book -d scratch/nonexistent2 localnonexistent2
  abort: infinitepush bookmark 'scratch/nonexistent2' does not exist in path 'default'
  [255]

Test deleting a nonexistent bookmark with an existing tag that has the right name
  $ hg book --remote
  $ hg book -d scratch/serverbranch
  abort: infinitepush bookmark 'scratch/serverbranch' does not exist in path 'default'
  [255]
  $ hg book --remote

Test deleting a local bookmark that has a scratch-like name
  $ hg book scratch/thisisalocalbm
  $ hg book
   * scratch/thisisalocalbm    1:45f7b362ad7c
  $ hg book --remote
  $ hg book -d scratch/thisisalocalbm
  $ hg book
  no bookmarks set
  $ hg book --remote

Prepare client to be pushed to for next tests
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server=yes
  > indextype=disk
  > storetype=disk
  > EOF

Test scratch bookmarks still pullable
  $ cd ..
  $ hg clone ssh://user@dummy/repo --config extensions.remotenames= client2 -q
  $ cd client2
  $ hg book -a
  no bookmarks set
  $ hg pull -B scratch/test1
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg book -a
  no bookmarks set
     default/scratch/test1     1:45f7b362ad7c
  $ hg up scratch/test1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls -a
  .
  ..
  .hg
  scratchcommit1
  servercommit

Add a second remote
  $ cat >> .hg/hgrc << EOF
  > [paths]
  > remote2 = ssh://user@dummy/client
  > EOF

Create some bookmarks on remote2
  $ mkcommit r2c
  $ hg push remote2 -r . --to scratch/realscratch2 --create
  pushing to ssh://user@dummy/client
  searching for changes
  remote: pushing 1 commit:
  remote:     7601bbca65fd  r2c
  $ hg book local2
  $ hg book -a
   * local2                    2:7601bbca65fd
     default/scratch/test1     1:45f7b362ad7c
     remote2/scratch/realscratch2 2:7601bbca65fd

Delete all the things !
  $ hg book -d --remote-path default scratch/test1
  $ hg book -a
   * local2                    2:7601bbca65fd
     remote2/scratch/realscratch2 2:7601bbca65fd
  $ hg book -d --remote-path nosuchremote scratch/realscratch2
  abort: repository nosuchremote does not exist!
  [255]
  $ hg book -a
   * local2                    2:7601bbca65fd
     remote2/scratch/realscratch2 2:7601bbca65fd
  $ hg book -d --remote-path remote2 scratch/realscratch2
  $ hg book -a
   * local2                    2:7601bbca65fd
  $ hg book -d local2
  $ hg book -a
  no bookmarks set

