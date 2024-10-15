#chg-compatible
#debugruntest-incompatible

  $ configure dummyssh
  $ enable commitcloud
  $ disable infinitepush
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
  $ hg book master
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
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client

Ensure no bookmarks
  $ hg book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
  $ hg book
  no bookmarks set

Push scratch bookmark
  $ mkcommit scratchcommit1
  $ hg push -qr . --to scratch/test1 --create
  $ hg book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
     remote/scratch/test1             45f7b362ad7cbaee8758e111c407f615dcd82f00

Delete scratch bookmark
  $ hg push -q --delete scratch/test1
  $ hg book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
  $ hg push -q --to scratch/test1 -r 45f7b362ad7c --create

Check regular deletion still works
  $ hg book testlocal1
  $ hg book
   * testlocal1                45f7b362ad7c
  $ hg book -d testlocal1
  $ hg book
  no bookmarks set

Test deleting both regular and scratch
  $ hg push -qr . --to scratch/test2 --create
  $ hg book testlocal2
  $ hg book -a
   * testlocal2                45f7b362ad7c
     remote/master             ac312cb08db5
     remote/scratch/test1      45f7b362ad7c
     remote/scratch/test2      45f7b362ad7c
  $ hg book -d testlocal2
  $ hg push -q --delete scratch/test2
  $ hg book -a
  no bookmarks set
     remote/master             ac312cb08db5
     remote/scratch/test1      45f7b362ad7c

Test deleting nonexistent bookmarks
  $ hg book -d scratch/nonexistent1
  abort: scratch bookmark 'scratch/nonexistent1' does not exist in path 'default'
  [255]
  $ hg book -d localnonexistent1
  abort: bookmark 'localnonexistent1' does not exist
  [255]
  $ hg book -d scratch/nonexistent2 localnonexistent2
  abort: scratch bookmark 'scratch/nonexistent2' does not exist in path 'default'
  [255]

Test deleting a nonexistent bookmark with an existing tag that has the right name
  $ hg book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
     remote/scratch/test1             45f7b362ad7cbaee8758e111c407f615dcd82f00
  $ hg book -d scratch/serverbranch
  abort: scratch bookmark 'scratch/serverbranch' does not exist in path 'default'
  [255]
  $ hg book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
     remote/scratch/test1             45f7b362ad7cbaee8758e111c407f615dcd82f00

Test deleting a local bookmark that has a scratch-like name
  $ hg book scratch/thisisalocalbm
  $ hg book
   * scratch/thisisalocalbm    45f7b362ad7c
  $ hg book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
     remote/scratch/test1             45f7b362ad7cbaee8758e111c407f615dcd82f00
  $ hg book -d scratch/thisisalocalbm
  $ hg book
  no bookmarks set
  $ hg book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
     remote/scratch/test1             45f7b362ad7cbaee8758e111c407f615dcd82f00

Prepare client to be pushed to for next tests
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server=yes
  > indextype=disk
  > storetype=disk
  > EOF

Test scratch bookmarks still pullable
  $ cd ..
  $ hg clone ssh://user@dummy/repo client2 -q
  $ cd client2
  $ hg book -a
  no bookmarks set
     remote/master             ac312cb08db5
  $ hg pull -B scratch/test1
  pulling from ssh://user@dummy/repo
  searching for changes
  $ hg book -a
  no bookmarks set
     remote/master             ac312cb08db5
     remote/scratch/test1      45f7b362ad7c
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
TODO: specifying remote doesn't work w/ SLAPI push
#if false
  $ mkcommit r2c
  $ hg push remote2 -r . --to scratch/realscratch2 --create
  pushing to ssh://user@dummy/client
  searching for changes
  remote: pushing 1 commit:
  remote:     7601bbca65fd  r2c
#endif

  $ hg book local2
  $ hg book -a
   * local2                    45f7b362ad7c
     remote/master             ac312cb08db5
     remote/scratch/test1      45f7b362ad7c

Delete all the things !
  $ hg book -d --remote-path nosuchremote scratch/test1
  abort: repository nosuchremote does not exist!
  [255]
  $ hg push -q --delete scratch/test1
  $ hg book -a
   * local2                    45f7b362ad7c
     remote/master             ac312cb08db5
  $ hg book -a
   * local2                    45f7b362ad7c
     remote/master             ac312cb08db5
  $ hg book -a
   * local2                    45f7b362ad7c
     remote/master             ac312cb08db5
  $ hg book -d local2
  $ hg book -a
  no bookmarks set
     remote/master             ac312cb08db5

