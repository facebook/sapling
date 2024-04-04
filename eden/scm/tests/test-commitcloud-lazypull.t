#debugruntest-compatible

#require no-eden

  $ setconfig experimental.allowfilepeer=True

  $ configure dummyssh mutation-norecord
  $ enable amend commitcloud infinitepush rebase remotenames share
  $ setconfig infinitepush.branchpattern="re:scratch/.*" commitcloud.hostname=testhost
  $ readconfig <<EOF
  > [alias]
  > trglog = log -G --template "{node|short} '{desc}' {bookmarks} {remotenames}\n"
  > descr = log -r '.' --template "{desc}"
  > EOF

  $ setconfig remotefilelog.reponame=server

  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg commit -Aqm "$1"
  > }

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server = yes
  > indextype = disk
  > storetype = disk
  > reponame = testrepo
  > EOF

  $ mkcommit "base"
  $ hg bookmark master
  $ cd ..

Make shared part of config
  $ cat >> shared.rc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > EOF

Make the first clone of the server
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ cat ../shared.rc >> .hg/hgrc
  $ hg cloud join -q

  $ cd ..

Make the second clone of the server
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat ../shared.rc >> .hg/hgrc
  $ hg cloud join -q

  $ cd ..

Test for `hg unamend`

Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "feature1"
  $ hg cloud sync
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

  $ hg amend -m "feature1 renamed"
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head 'b68dd726c6c6' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 0 trees for upload
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Sync from the second client and `hg unamend` there
  $ cd client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling b68dd726c6c6 from ssh://user@dummy/server
  searching for changes
  fetching revlog data for 1 commits
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglog
  o  b68dd726c6c6 'feature1 renamed'
  │
  @  d20a80d4def3 'base'
  

  $ hg up b68dd726c6c6
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg unamend
  pulling '1cf4a5a0e8fc41ef1289e833ebdb22d754c080ac' from 'ssh://user@dummy/server'

  $ tglog
  @  1cf4a5a0e8fc 'feature1'
  │
  o  d20a80d4def3 'base'
  

(with mutation and visibility, it's not possible to undo the relationship of
amend, therefore the "has been replaced" message)
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

  $ cd client1

  $ hg cloud sync
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
  $ hg up -q 1cf4a5a0e8fc
  $ hg amend -m "feature1 renamed2"
  $ hg amend -m "feature1 renamed3"
  $ hg unamend
  $ hg unhide 74b668b6b779
  $ tglog
  o  74b668b6b779 'feature1 renamed3'
  │
  │ @  cb45bbd0ae75 'feature1 renamed2'
  ├─╯
  o  d20a80d4def3 'base'
  
  $ P=1 hg cloud sync
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
  hint[commitcloud-update-on-move]: if you would like to update to the moved version automatically, run:
  `hg config --user commitcloud.updateonmove=true`
  hint[hint-ack]: use 'hg hint --ack commitcloud-update-on-move' to silence these hints

Now cloud sync in the other client.  The cycle means we can't reliably pick a destination.
  $ cd ../client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling cb45bbd0ae75 74b668b6b779 from ssh://user@dummy/server
  searching for changes
  fetching revlog data for 2 commits
  commitcloud: commits synchronized
  finished in * sec (glob)
  commitcloud: current revision 1cf4a5a0e8fc has been moved remotely to 74b668b6b779
  hint[commitcloud-update-on-move]: if you would like to update to the moved version automatically, run:
  `hg config --user commitcloud.updateonmove=true`
  hint[hint-ack]: use 'hg hint --ack commitcloud-update-on-move' to silence these hints
