#chg-compatible
#require no-eden

  $ configure dummyssh
  $ enable commitcloud
  $ disable infinitepush
  $ setconfig infinitepush.branchpattern="re:scratch/.+"
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    sl add "$1"
  >    sl ci -m "$1"
  > }

Create server repo
  $ sl init repo
  $ cd repo
  $ mkcommit servercommit
  $ cat >> .sl/config << EOF
  > [infinitepush]
  > server=yes
  > indextype=disk
  > storetype=disk
  > EOF
  $ sl book master
  $ cd ..

Create second server repo
  $ sl init repo2
  $ cd repo2
  $ mkcommit servercommit2
  $ cat >> .sl/config << EOF
  > [infinitepush]
  > server=yes
  > indextype=disk
  > storetype=disk
  > EOF
  $ cd ..

Clone server
  $ sl clone ssh://user@dummy/repo client -q
  $ cd client

Ensure no bookmarks
  $ sl book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
  $ sl book
  no bookmarks set

Push scratch bookmark
  $ mkcommit scratchcommit1
  $ sl push -qr . --to scratch/test1 --create
  $ sl book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
     remote/scratch/test1             45f7b362ad7cbaee8758e111c407f615dcd82f00

Delete scratch bookmark
  $ sl push -q --delete scratch/test1
  $ sl book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
  $ sl push -q --to scratch/test1 -r 45f7b362ad7c --create

Check regular deletion still works
  $ sl book testlocal1
  $ sl book
   * testlocal1                45f7b362ad7c
  $ sl book -d testlocal1
  $ sl book
  no bookmarks set

Test deleting both regular and scratch
  $ sl push -qr . --to scratch/test2 --create
  $ sl book testlocal2
  $ sl book -a
   * testlocal2                45f7b362ad7c
     remote/master             ac312cb08db5
     remote/scratch/test1      45f7b362ad7c
     remote/scratch/test2      45f7b362ad7c
  $ sl book -d testlocal2
  $ sl push -q --delete scratch/test2
  $ sl book -a
  no bookmarks set
     remote/master             ac312cb08db5
     remote/scratch/test1      45f7b362ad7c

Test deleting nonexistent bookmarks
  $ sl book -d scratch/nonexistent1
  abort: scratch bookmark 'scratch/nonexistent1' does not exist in path 'default'
  [255]
  $ sl book -d localnonexistent1
  abort: bookmark 'localnonexistent1' does not exist
  [255]
  $ sl book -d scratch/nonexistent2 localnonexistent2
  abort: scratch bookmark 'scratch/nonexistent2' does not exist in path 'default'
  [255]

Test deleting a nonexistent bookmark with an existing tag that has the right name
  $ sl book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
     remote/scratch/test1             45f7b362ad7cbaee8758e111c407f615dcd82f00
  $ sl book -d scratch/serverbranch
  abort: scratch bookmark 'scratch/serverbranch' does not exist in path 'default'
  [255]
  $ sl book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
     remote/scratch/test1             45f7b362ad7cbaee8758e111c407f615dcd82f00

Test deleting a local bookmark that has a scratch-like name
  $ sl book scratch/thisisalocalbm
  $ sl book
   * scratch/thisisalocalbm    45f7b362ad7c
  $ sl book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
     remote/scratch/test1             45f7b362ad7cbaee8758e111c407f615dcd82f00
  $ sl book -d scratch/thisisalocalbm
  $ sl book
  no bookmarks set
  $ sl book --remote
     remote/master                    ac312cb08db5366e622a01fd001e583917eb9f1c
     remote/scratch/test1             45f7b362ad7cbaee8758e111c407f615dcd82f00

Prepare client to be pushed to for next tests
  $ cat >> .sl/config << EOF
  > [infinitepush]
  > server=yes
  > indextype=disk
  > storetype=disk
  > EOF

Test scratch bookmarks still pullable
  $ cd ..
  $ sl clone ssh://user@dummy/repo client2 -q
  $ cd client2
  $ sl book -a
  no bookmarks set
     remote/master             ac312cb08db5
  $ sl pull -B scratch/test1
  pulling from ssh://user@dummy/repo
  searching for changes
  $ sl book -a
  no bookmarks set
     remote/master             ac312cb08db5
     remote/scratch/test1      45f7b362ad7c
  $ sl up scratch/test1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls | sort
  scratchcommit1
  servercommit

Add a second remote
  $ cat >> .sl/config << EOF
  > [paths]
  > remote2 = ssh://user@dummy/client
  > EOF

Create some bookmarks on remote2
TODO: specifying remote doesn't work w/ SLAPI push
#if false
  $ mkcommit r2c
  $ sl push remote2 -r . --to scratch/realscratch2 --create
  pushing to ssh://user@dummy/client
  searching for changes
  remote: pushing 1 commit:
  remote:     7601bbca65fd  r2c
#endif

  $ sl book local2
  $ sl book -a
   * local2                    45f7b362ad7c
     remote/master             ac312cb08db5
     remote/scratch/test1      45f7b362ad7c

Delete all the things !
  $ sl book -d --remote-path nosuchremote scratch/test1
  abort: repository nosuchremote does not exist!
  [255]
  $ sl push -q --delete scratch/test1
  $ sl book -a
   * local2                    45f7b362ad7c
     remote/master             ac312cb08db5
  $ sl book -a
   * local2                    45f7b362ad7c
     remote/master             ac312cb08db5
  $ sl book -a
   * local2                    45f7b362ad7c
     remote/master             ac312cb08db5
  $ sl book -d local2
  $ sl book -a
  no bookmarks set
     remote/master             ac312cb08db5
