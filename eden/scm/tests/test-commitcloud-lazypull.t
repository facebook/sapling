#modern-config-incompatible

#require no-eden


  $ configure dummyssh mutation-norecord
  $ enable amend commitcloud rebase share
  $ setconfig commit.modify-obsolete-mode=ignore
  $ setconfig infinitepush.branchpattern="re:scratch/.*" commitcloud.hostname=testhost
  $ readconfig <<EOF
  > [alias]
  > trglog = log -G --template "{node|short} '{desc}' {bookmarks} {remotenames}\n"
  > descr = log -r '.' --template "{desc}"
  > EOF

  $ setconfig remotefilelog.reponame=server

  $ mkcommit() {
  >   echo "$1" > "$1"
  >   sl commit -Aqm "$1"
  > }

  $ sl init server
  $ cd server
  $ cat >> .sl/config << EOF
  > [infinitepush]
  > server = yes
  > indextype = disk
  > storetype = disk
  > reponame = testrepo
  > EOF

  $ mkcommit "base"
  $ sl bookmark master
  $ cd ..

Make shared part of config
  $ cat >> shared.rc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > EOF

Make the first clone of the server
  $ sl clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ cat ../shared.rc >> .sl/config
  $ sl cloud join -q

  $ cd ..

Make the second clone of the server
  $ sl clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat ../shared.rc >> .sl/config
  $ sl cloud join -q

  $ cd ..

Test for `sl unamend`

Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "feature1"
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head '1cf4a5a0e8fc' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

  $ sl amend -m "feature1 renamed"
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head 'b68dd726c6c6' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 0 trees for upload
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Sync from the second client and `sl unamend` there
  $ cd client2
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling b68dd726c6c6 from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglog
  o  b68dd726c6c6 'feature1 renamed'
  │
  @  d20a80d4def3 'base'
  

  $ sl up b68dd726c6c6
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ sl unamend
  pulling '1cf4a5a0e8fc41ef1289e833ebdb22d754c080ac' from 'ssh://user@dummy/server'

  $ tglog
  @  1cf4a5a0e8fc 'feature1'
  │
  o  d20a80d4def3 'base'
  

(with mutation and visibility, it's not possible to undo the relationship of
amend, therefore the "has been replaced" message)
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

  $ cd client1

  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  @  b68dd726c6c6 'feature1 renamed'
  │
  │ x  1cf4a5a0e8fc 'feature1'
  ├─╯
  o  d20a80d4def3 'base'
  
Amend twice, unamend, then unhide
  $ sl up -q 1cf4a5a0e8fc
  $ sl amend -m "feature1 renamed2"
  $ sl amend -m "feature1 renamed3"
  $ sl unamend
  $ sl unhide 74b668b6b779
  $ tglog
  o  74b668b6b779 'feature1 renamed3'
  │
  │ @  cb45bbd0ae75 'feature1 renamed2'
  ├─╯
  o  d20a80d4def3 'base'
  
  $ P=1 sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head 'cb45bbd0ae75' hasn't been uploaded yet
  commitcloud: head '74b668b6b779' hasn't been uploaded yet
  edenapi: queue 2 commits for upload
  edenapi: queue 0 files for upload
  edenapi: queue 0 trees for upload
  edenapi: uploaded 2 changesets
  commitcloud: commits synchronized
  finished in * sec (glob)
  commitcloud: current revision cb45bbd0ae75 has been moved remotely to 74b668b6b779

Now cloud sync in the other client.  The cycle means we can't reliably pick a destination.
  $ cd ../client2
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling cb45bbd0ae75 74b668b6b779 from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * sec (glob)
  commitcloud: current revision 1cf4a5a0e8fc has been moved remotely to 74b668b6b779
