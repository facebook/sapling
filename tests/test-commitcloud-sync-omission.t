#require jq
  $ setconfig extensions.treemanifest=!
  $ enable smartlog

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend =
  > commitcloud =
  > infinitepush =
  > rebase =
  > remotenames =
  > share =
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > [infinitepush]
  > branchpattern = re:scratch/.*
  > [infinitepushbackup]
  > enablestatus = true
  > [commitcloud]
  > hostname = testhost
  > max_sync_age = 14
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF

  $ setconfig remotefilelog.reponame=server

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server = yes
  > indextype = disk
  > storetype = disk
  > reponame = testrepo
  > EOF
  $ touch base
  $ hg commit -Aqm base
  $ hg phase -p .
  $ cd ..

Make shared part of config
  $ cat >> shared.rc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > user_token_path = $TESTTMP
  > auth_help = visit https://localhost/oauth to generate a registration token
  > education_page = https://someurl.com/wiki/CommitCloud
  > owner_team = The Test Team @ FB
  > EOF

Utility script to dump commit cloud metadata
  $ cat > dumpcommitcloudmetadata.py <<EOF
  > import json
  > ccmd = json.load(open("$TESTTMP/commitcloudservicedb"))
  > print("version: %s" % ccmd["version"])
  > print("bookmarks:")
  > for bookmark, node in sorted(ccmd["bookmarks"].items()):
  >    print("    %s => %s" % (bookmark, node))
  > print("heads:")
  > for head in (ccmd["heads"]):
  >    print("    %s" % head)
  > EOF

Utility function to run HG with a fake date
  $ hgfakedate() {
  >   fakedate="$1"
  >   shift
  >   hg --config extensions.fakedate="$TESTDIR/fakedate.py" --config fakedate.date="$fakedate" "$@"
  > }

Make a clone of the server
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ cat ../shared.rc >> .hg/hgrc
  $ hg cloud auth -t xxxxxx
  setting authentication token
  authentication successful

Connect the first client
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

Make some stacks with various dates.  We will use Feb 1990 for these tests.

- Old stack
  $ hg up -q 0
  $ touch oldstack-feb1
  $ hg commit -Aqm oldstack-feb1 --config devel.default-date="1990-02-01T00:00Z"
  $ hg book -i oldbook
  $ touch oldstack-feb4
  $ hg commit -Aqm oldstack-feb4 --config devel.default-date="1990-02-04T12:00Z"

- Middle stack
  $ hg up -q 0
  $ touch midstack-feb7
  $ hg commit -Aqm midstack-feb7 --config devel.default-date="1990-02-07T00:00Z"
  $ hg book -i midbook
  $ touch midstack-feb9
  $ hg commit -Aqm midstack-feb9 --config devel.default-date="1990-02-09T12:00Z"

- New stack
  $ hg up -q 0
  $ touch newstack-feb13
  $ hg commit -Aqm newstack-feb13 --config devel.default-date="1990-02-13T00:00Z"
  $ hg book -i newbook
  $ touch newstack-feb15
  $ hg commit -Aqm newstack-feb15 --config devel.default-date="1990-02-15T12:00Z"

Write node metadata out to disk (this is loaded by the commit cloud local service
implementation)
  $ hg log -r "all()" -Tjson >> $TESTTMP/nodedata

Sync these to commit cloud - they all get pushed even though they are old
  $ hgfakedate 1990-02-28T00:00Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 1f9ebd6d1390
  remote: pushing 2 commits:
  remote:     1f9ebd6d1390  oldstack-feb1
  remote:     d16408588b2d  oldstack-feb4
  backing up stack rooted at 1c1b7955142c
  remote: pushing 2 commits:
  remote:     1c1b7955142c  midstack-feb7
  remote:     d133b886da68  midstack-feb9
  backing up stack rooted at 56a352317b67
  remote: pushing 2 commits:
  remote:     56a352317b67  newstack-feb13
  remote:     7f958333fe84  newstack-feb15
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglogp
  @  6: 7f958333fe84 draft 'newstack-feb15'
  |
  o  5: 56a352317b67 draft 'newstack-feb13' newbook
  |
  | o  4: d133b886da68 draft 'midstack-feb9'
  | |
  | o  3: 1c1b7955142c draft 'midstack-feb7' midbook
  |/
  | o  2: d16408588b2d draft 'oldstack-feb4'
  | |
  | o  1: 1f9ebd6d1390 draft 'oldstack-feb1' oldbook
  |/
  o  0: df4f53cec30a public 'base'
  

  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 2
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      newbook => 56a352317b67ae3d5abd5f6c71ec0df3aa98fe97
      oldbook => 1f9ebd6d1390ebc603e401171eda0c444a0f8754
  heads:
      d16408588b2d047410f99c45e425bf97923e28f2
      d133b886da6874fe25998d26ae1b2b8528b07c59
      7f958333fe845fe5cbc60d9d96e3d68a262d684c

Create a new client that isn't connected yet
  $ cd ..
  $ hg clone ssh://user@dummy/server client2 -q
  $ cat shared.rc >> client2/.hg/hgrc

Connect to commit cloud
  $ cd client2
  $ hgfakedate 1990-02-20T16:00Z cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting heads that are older than 14 days:
    d16408588b2d from Sun Feb 04 12:00:00 1990 +0000
  pulling d133b886da68
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  pulling 7f958333fe84
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  1f9ebd6d1390ebc603e401171eda0c444a0f8754 not found, omitting oldbook bookmark
  new changesets 1c1b7955142c:7f958333fe84
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglogp
  o  4: 7f958333fe84 draft 'newstack-feb15'
  |
  o  3: 56a352317b67 draft 'newstack-feb13' newbook
  |
  | o  2: d133b886da68 draft 'midstack-feb9'
  | |
  | o  1: 1c1b7955142c draft 'midstack-feb7' midbook
  |/
  @  0: df4f53cec30a public 'base'
  

Create a new commit
  $ hg up 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch client2-file1
  $ hg commit -Aqm client2-feb28 --config devel.default-date="1990-02-28T01:00Z"
  $ (cat $TESTTMP/nodedata ; hg log -r . -Tjson) | jq -s add > $TESTTMP/nodedata.new
  $ mv $TESTTMP/nodedata.new $TESTTMP/nodedata
  $ hgfakedate 1990-02-28T01:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at ff52de2f760c
  remote: pushing 1 commit:
  remote:     ff52de2f760c  client2-feb28
  commitcloud: commits synchronized
  finished in * (glob)

Sync these commits to the first client - it has everything
  $ cd ../client1
  $ hgfakedate 1990-02-28T01:02Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling ff52de2f760c
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets ff52de2f760c
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  7: ff52de2f760c draft 'client2-feb28'
  |
  | @  6: 7f958333fe84 draft 'newstack-feb15'
  | |
  | o  5: 56a352317b67 draft 'newstack-feb13' newbook
  |/
  | o  4: d133b886da68 draft 'midstack-feb9'
  | |
  | o  3: 1c1b7955142c draft 'midstack-feb7' midbook
  |/
  | o  2: d16408588b2d draft 'oldstack-feb4'
  | |
  | o  1: 1f9ebd6d1390 draft 'oldstack-feb1' oldbook
  |/
  o  0: df4f53cec30a public 'base'
  

Second client can still sync
  $ cd ../client2
  $ hgfakedate 1990-02-28T01:22Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  @  5: ff52de2f760c draft 'client2-feb28'
  |
  | o  4: 7f958333fe84 draft 'newstack-feb15'
  | |
  | o  3: 56a352317b67 draft 'newstack-feb13' newbook
  |/
  | o  2: d133b886da68 draft 'midstack-feb9'
  | |
  | o  1: 1c1b7955142c draft 'midstack-feb7' midbook
  |/
  o  0: df4f53cec30a public 'base'
  

  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 3
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      newbook => 56a352317b67ae3d5abd5f6c71ec0df3aa98fe97
      oldbook => 1f9ebd6d1390ebc603e401171eda0c444a0f8754
  heads:
      d16408588b2d047410f99c45e425bf97923e28f2
      d133b886da6874fe25998d26ae1b2b8528b07c59
      7f958333fe845fe5cbc60d9d96e3d68a262d684c
      ff52de2f760c67fa6f89273ca7f770396a3c81c4

Add a new commit to a stack on the first client
  $ cd ../client1
  $ touch newstack-feb28
  $ hg commit -Aqm newstack-feb28 --config devel.default-date="1990-02-28T02:00Z"
  $ (cat $TESTTMP/nodedata ; hg log -r . -Tjson) | jq -s add > $TESTTMP/nodedata.new
  $ mv $TESTTMP/nodedata.new $TESTTMP/nodedata

  $ hgfakedate 1990-02-28T02:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 56a352317b67
  remote: pushing 3 commits:
  remote:     56a352317b67  newstack-feb13
  remote:     7f958333fe84  newstack-feb15
  remote:     46f8775ee5d4  newstack-feb28
  commitcloud: commits synchronized
  finished in * (glob)

  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 4
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      newbook => 56a352317b67ae3d5abd5f6c71ec0df3aa98fe97
      oldbook => 1f9ebd6d1390ebc603e401171eda0c444a0f8754
  heads:
      d16408588b2d047410f99c45e425bf97923e28f2
      d133b886da6874fe25998d26ae1b2b8528b07c59
      ff52de2f760c67fa6f89273ca7f770396a3c81c4
      46f8775ee5d479eed945b5186929bd046f116176

Second client syncs that in, but still leaves the old commits missing
  $ cd ../client2
  $ hgfakedate 1990-02-28T02:02Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting heads that are older than 14 days:
    d16408588b2d from Sun Feb 04 12:00:00 1990 +0000
  pulling 46f8775ee5d4
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files
  new changesets 46f8775ee5d4
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  6: 46f8775ee5d4 draft 'newstack-feb28'
  |
  | @  5: ff52de2f760c draft 'client2-feb28'
  | |
  o |  4: 7f958333fe84 draft 'newstack-feb15'
  | |
  o |  3: 56a352317b67 draft 'newstack-feb13' newbook
  |/
  | o  2: d133b886da68 draft 'midstack-feb9'
  | |
  | o  1: 1c1b7955142c draft 'midstack-feb7' midbook
  |/
  o  0: df4f53cec30a public 'base'
  

  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 4
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      newbook => 56a352317b67ae3d5abd5f6c71ec0df3aa98fe97
      oldbook => 1f9ebd6d1390ebc603e401171eda0c444a0f8754
  heads:
      d16408588b2d047410f99c45e425bf97923e28f2
      d133b886da6874fe25998d26ae1b2b8528b07c59
      ff52de2f760c67fa6f89273ca7f770396a3c81c4
      46f8775ee5d479eed945b5186929bd046f116176

Commit cloud keeps infinitepush backup state up-to-date.  Ensure it hasn't included the omitted head.
  $ grep -r d16408588b2d047410f99c45e425bf97923e28f2 .hg/infinitepushbackups
  [1]

First client add a new commit to the old stack
  $ cd ../client1
  $ hg up 2
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved

  $ touch oldstack-mar4
  $ hg commit -Aqm oldstack-mar4 --config devel.default-date="1990-03-04T03:00Z"
  $ (cat $TESTTMP/nodedata ; hg log -r . -Tjson) | jq -s add > $TESTTMP/nodedata.new
  $ mv $TESTTMP/nodedata.new $TESTTMP/nodedata
  $ hgfakedate 1990-03-04T03:02Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 1f9ebd6d1390
  remote: pushing 3 commits:
  remote:     1f9ebd6d1390  oldstack-feb1
  remote:     d16408588b2d  oldstack-feb4
  remote:     2b8dce7bd745  oldstack-mar4
  commitcloud: commits synchronized
  finished in * (glob)

  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 5
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      newbook => 56a352317b67ae3d5abd5f6c71ec0df3aa98fe97
      oldbook => 1f9ebd6d1390ebc603e401171eda0c444a0f8754
  heads:
      d133b886da6874fe25998d26ae1b2b8528b07c59
      ff52de2f760c67fa6f89273ca7f770396a3c81c4
      46f8775ee5d479eed945b5186929bd046f116176
      2b8dce7bd745e54f2cea9d8c630d97264537cbad

Second client syncs the old stack in, and now has the bookmark
  $ cd ../client2
  $ hgfakedate 1990-03-04T03:03Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 2b8dce7bd745
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+1 heads)
  new changesets 1f9ebd6d1390:2b8dce7bd745
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  9: 2b8dce7bd745 draft 'oldstack-mar4'
  |
  o  8: d16408588b2d draft 'oldstack-feb4'
  |
  o  7: 1f9ebd6d1390 draft 'oldstack-feb1' oldbook
  |
  | o  6: 46f8775ee5d4 draft 'newstack-feb28'
  | |
  +---@  5: ff52de2f760c draft 'client2-feb28'
  | |
  | o  4: 7f958333fe84 draft 'newstack-feb15'
  | |
  | o  3: 56a352317b67 draft 'newstack-feb13' newbook
  |/
  | o  2: d133b886da68 draft 'midstack-feb9'
  | |
  | o  1: 1c1b7955142c draft 'midstack-feb7' midbook
  |/
  o  0: df4f53cec30a public 'base'
  
  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 5
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      newbook => 56a352317b67ae3d5abd5f6c71ec0df3aa98fe97
      oldbook => 1f9ebd6d1390ebc603e401171eda0c444a0f8754
  heads:
      d133b886da6874fe25998d26ae1b2b8528b07c59
      ff52de2f760c67fa6f89273ca7f770396a3c81c4
      46f8775ee5d479eed945b5186929bd046f116176
      2b8dce7bd745e54f2cea9d8c630d97264537cbad

Create a new client that isn't connected yet
  $ cd ..
  $ hg clone ssh://user@dummy/server client3 -q
  $ cat shared.rc >> client3/.hg/hgrc

Connect to commit cloud
  $ cd client3
  $ hgfakedate 1990-03-05T12:00Z cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting heads that are older than 14 days:
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
  pulling ff52de2f760c 46f8775ee5d4
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+1 heads)
  pulling 2b8dce7bd745
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+1 heads)
  1c1b7955142cd8a3beec705c9cca9d775ecb0fa8 not found, omitting midbook bookmark
  new changesets ff52de2f760c:2b8dce7bd745
  commitcloud: commits synchronized
  finished in * (glob)

  $ hgfakedate 1990-03-05T12:00Z smartlog -T '{rev}: {node|short} {desc} {bookmarks}' --config infinitepushbackup.autobackup=true
  o  7: 2b8dce7bd745 oldstack-mar4
  |
  o  6: d16408588b2d oldstack-feb4
  |
  o  5: 1f9ebd6d1390 oldstack-feb1 oldbook
  |
  | o  4: 46f8775ee5d4 newstack-feb28
  | |
  | o  3: 7f958333fe84 newstack-feb15
  | |
  | o  2: 56a352317b67 newstack-feb13 newbook
  |/
  | o  1: ff52de2f760c client2-feb28
  |/
  @  0: df4f53cec30a base
  
  hint[commitcloud-old-commits]: some older commits or bookmarks have not been synced to this repo
  (run 'hg cloud sl' to see all of the commits in your workspace)
  (run 'hg pull -r HASH' to fetch commits by hash)
  (run 'hg cloud sync --full' to fetch everything - this may be slow)
  hint[hint-ack]: use 'hg hint --ack commitcloud-old-commits' to silence these hints

Move one of these bookmarks in the first client.

  $ cd ../client1
  $ hg book -f -r 4 oldbook
  $ hgfakedate 1990-03-05T12:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

Do a sync in the new client - the bookmark is left where it was

  $ cd ../client3
  $ hgfakedate 1990-03-05T12:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting heads that are older than 14 days:
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
  d133b886da6874fe25998d26ae1b2b8528b07c59 not found, omitting oldbook bookmark
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  7: 2b8dce7bd745 draft 'oldstack-mar4'
  |
  o  6: d16408588b2d draft 'oldstack-feb4'
  |
  o  5: 1f9ebd6d1390 draft 'oldstack-feb1'
  |
  | o  4: 46f8775ee5d4 draft 'newstack-feb28'
  | |
  | o  3: 7f958333fe84 draft 'newstack-feb15'
  | |
  | o  2: 56a352317b67 draft 'newstack-feb13' newbook
  |/
  | o  1: ff52de2f760c draft 'client2-feb28'
  |/
  @  0: df4f53cec30a public 'base'
  
Move the bookmark locally - this still gets synced ok.

  $ hg book -f -r 3 oldbook
  $ tglogp
  o  7: 2b8dce7bd745 draft 'oldstack-mar4'
  |
  o  6: d16408588b2d draft 'oldstack-feb4'
  |
  o  5: 1f9ebd6d1390 draft 'oldstack-feb1'
  |
  | o  4: 46f8775ee5d4 draft 'newstack-feb28'
  | |
  | o  3: 7f958333fe84 draft 'newstack-feb15' oldbook
  | |
  | o  2: 56a352317b67 draft 'newstack-feb13' newbook
  |/
  | o  1: ff52de2f760c draft 'client2-feb28'
  |/
  @  0: df4f53cec30a public 'base'
  
  $ hgfakedate 1990-03-05T12:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ../client1
  $ hgfakedate 1990-03-05T12:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  @  9: 2b8dce7bd745 draft 'oldstack-mar4'
  |
  | o  8: 46f8775ee5d4 draft 'newstack-feb28'
  | |
  | | o  7: ff52de2f760c draft 'client2-feb28'
  | | |
  | o |  6: 7f958333fe84 draft 'newstack-feb15' oldbook
  | | |
  | o |  5: 56a352317b67 draft 'newstack-feb13' newbook
  | |/
  | | o  4: d133b886da68 draft 'midstack-feb9'
  | | |
  | | o  3: 1c1b7955142c draft 'midstack-feb7' midbook
  | |/
  o |  2: d16408588b2d draft 'oldstack-feb4'
  | |
  o |  1: 1f9ebd6d1390 draft 'oldstack-feb1'
  |/
  o  0: df4f53cec30a public 'base'
  

A full sync pulls the old commits in
  $ cd ../client3
  $ hgfakedate 1990-03-05T12:01Z cloud sync --full
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling d133b886da68
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  new changesets 1c1b7955142c:d133b886da68
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglogp
  o  9: d133b886da68 draft 'midstack-feb9'
  |
  o  8: 1c1b7955142c draft 'midstack-feb7' midbook
  |
  | o  7: 2b8dce7bd745 draft 'oldstack-mar4'
  | |
  | o  6: d16408588b2d draft 'oldstack-feb4'
  | |
  | o  5: 1f9ebd6d1390 draft 'oldstack-feb1'
  |/
  | o  4: 46f8775ee5d4 draft 'newstack-feb28'
  | |
  | o  3: 7f958333fe84 draft 'newstack-feb15' oldbook
  | |
  | o  2: 56a352317b67 draft 'newstack-feb13' newbook
  |/
  | o  1: ff52de2f760c draft 'client2-feb28'
  |/
  @  0: df4f53cec30a public 'base'
  
Create a new client that isn't connected yet
  $ cd ..
  $ hg clone ssh://user@dummy/server client4 -q
  $ cat shared.rc >> client4/.hg/hgrc

A part sync omitting everything
  $ cd ./client4
  $ hgfakedate 1990-04-01T12:01Z cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting heads that are older than 14 days:
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
    ff52de2f760c from Wed Feb 28 01:00:00 1990 +0000
    46f8775ee5d4 from Wed Feb 28 02:00:00 1990 +0000
    2b8dce7bd745 from Sun Mar 04 03:00:00 1990 +0000
  * not found, omitting * bookmark (glob)
  * not found, omitting * bookmark (glob)
  * not found, omitting * bookmark (glob)
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  @  0: df4f53cec30a public 'base'
  
Remove some of the bookmarks
  $ cd ../client1
  $ hg book --delete newbook
  $ hg book --delete oldbook
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

Check that it doesn't break cloud sync
  $ cd ../client4
  $ hgfakedate 1990-04-01T12:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting heads that are older than 14 days:
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
    ff52de2f760c from Wed Feb 28 01:00:00 1990 +0000
    46f8775ee5d4 from Wed Feb 28 02:00:00 1990 +0000
    2b8dce7bd745 from Sun Mar 04 03:00:00 1990 +0000
  commitcloud: commits synchronized
  finished in * (glob)

Pull in some of the commits by setting max age manually
  $ hgfakedate 1990-04-01T12:01Z cloud sync --config commitcloud.max_sync_age=30
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting heads that are older than 30 days:
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
    ff52de2f760c from Wed Feb 28 01:00:00 1990 +0000
    46f8775ee5d4 from Wed Feb 28 02:00:00 1990 +0000
  pulling 2b8dce7bd745
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  new changesets 1f9ebd6d1390:2b8dce7bd745
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  3: 2b8dce7bd745 draft 'oldstack-mar4'
  |
  o  2: d16408588b2d draft 'oldstack-feb4'
  |
  o  1: 1f9ebd6d1390 draft 'oldstack-feb1'
  |
  @  0: df4f53cec30a public 'base'
  

Create a bookmark with the same name as an omitted bookmark
  $ hg book -r tip midbook
  $ hgfakedate 1990-04-01T12:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting heads that are older than 14 days:
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
    ff52de2f760c from Wed Feb 28 01:00:00 1990 +0000
    46f8775ee5d4 from Wed Feb 28 02:00:00 1990 +0000
  commitcloud: commits synchronized
  finished in * (glob)

Sync these changes to client3 - the deleted bookmarks are removed and the
other bookmark is treated like a move.
  $ cd ../client3
  $ hgfakedate 1990-04-01T12:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  9: d133b886da68 draft 'midstack-feb9'
  |
  o  8: 1c1b7955142c draft 'midstack-feb7'
  |
  | o  7: 2b8dce7bd745 draft 'oldstack-mar4' midbook
  | |
  | o  6: d16408588b2d draft 'oldstack-feb4'
  | |
  | o  5: 1f9ebd6d1390 draft 'oldstack-feb1'
  |/
  | o  4: 46f8775ee5d4 draft 'newstack-feb28'
  | |
  | o  3: 7f958333fe84 draft 'newstack-feb15'
  | |
  | o  2: 56a352317b67 draft 'newstack-feb13'
  |/
  | o  1: ff52de2f760c draft 'client2-feb28'
  |/
  @  0: df4f53cec30a public 'base'
  

In client1 (which hasn't synced yet), make the midbook commit obsolete.
  $ cd ../client1
  $ hg up -q 2b8dce7bd745
  $ hg amend -m "oldstack-mar4 amended"

Attempt to sync.  The midbook bookmark should make it visible again.
  $ hg cloud sync -q
  $ tglog
  @  10: 2ace67ee4791 'oldstack-mar4 amended'
  |
  | x  9: 2b8dce7bd745 'oldstack-mar4' midbook
  |/
  | o  8: 46f8775ee5d4 'newstack-feb28'
  | |
  | | o  7: ff52de2f760c 'client2-feb28'
  | | |
  | o |  6: 7f958333fe84 'newstack-feb15'
  | | |
  | o |  5: 56a352317b67 'newstack-feb13'
  | |/
  | | o  4: d133b886da68 'midstack-feb9'
  | | |
  | | o  3: 1c1b7955142c 'midstack-feb7'
  | |/
  o |  2: d16408588b2d 'oldstack-feb4'
  | |
  o |  1: 1f9ebd6d1390 'oldstack-feb1'
  |/
  o  0: df4f53cec30a 'base'
  
Sync in client2.  It should match.
  $ cd ../client2
  $ hg cloud sync -q
  $ tglog
  o  10: 2ace67ee4791 'oldstack-mar4 amended'
  |
  | x  9: 2b8dce7bd745 'oldstack-mar4' midbook
  |/
  o  8: d16408588b2d 'oldstack-feb4'
  |
  o  7: 1f9ebd6d1390 'oldstack-feb1'
  |
  | o  6: 46f8775ee5d4 'newstack-feb28'
  | |
  +---@  5: ff52de2f760c 'client2-feb28'
  | |
  | o  4: 7f958333fe84 'newstack-feb15'
  | |
  | o  3: 56a352317b67 'newstack-feb13'
  |/
  | o  2: d133b886da68 'midstack-feb9'
  | |
  | o  1: 1c1b7955142c 'midstack-feb7'
  |/
  o  0: df4f53cec30a 'base'
  
Hide some uninteresting commits and sync everywhere
  $ hg hide -r 1:: -r 3:: -r 5::
  hiding commit 1c1b7955142c "midstack-feb7"
  hiding commit d133b886da68 "midstack-feb9"
  hiding commit 56a352317b67 "newstack-feb13"
  hiding commit 7f958333fe84 "newstack-feb15"
  hiding commit ff52de2f760c "client2-feb28"
  hiding commit 46f8775ee5d4 "newstack-feb28"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at df4f53cec30a
  6 changesets hidden
  $ hg cloud sync -q
  $ cd ../client1
  $ hg cloud sync -q

Make a new public commit
  $ cd ../server
  $ echo public1 > public1
  $ hg commit -Aqm public1
  $ hg phase -p .

Pull this into client1
  $ cd ../client1
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets f770b7f72fa5

Move midbook to the public commit.
  $ hg book -fr 11 midbook
  $ hg cloud sync -q

Sync in client 2.  It doesn't have the new destination of midbook, so should omit it.

  $ cd ../client2
  $ hg cloud sync -q
  f770b7f72fa59cf01503318ed2b26904cb255d03 not found, omitting midbook bookmark
  $ tglogp
  o  10: 2ace67ee4791 draft 'oldstack-mar4 amended'
  |
  | x  9: 2b8dce7bd745 draft 'oldstack-mar4'
  |/
  o  8: d16408588b2d draft 'oldstack-feb4'
  |
  o  7: 1f9ebd6d1390 draft 'oldstack-feb1'
  |
  @  0: df4f53cec30a public 'base'
  
  $ cd ../client1
  $ hg cloud sync -q
  $ tglogp
  o  11: f770b7f72fa5 public 'public1' midbook
  |
  | @  10: 2ace67ee4791 draft 'oldstack-mar4 amended'
  | |
  | | x  9: 2b8dce7bd745 draft 'oldstack-mar4'
  | |/
  | o  2: d16408588b2d draft 'oldstack-feb4'
  | |
  | o  1: 1f9ebd6d1390 draft 'oldstack-feb1'
  |/
  o  0: df4f53cec30a public 'base'
  
Sync in client 4.  Some of the omitted heads in this client have been removed
from the cloud workspace, but the sync should still work.

  $ cd ../client4
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 2ace67ee4791
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 3 files (+1 heads)
  f770b7f72fa59cf01503318ed2b26904cb255d03 not found, omitting midbook bookmark
  transaction abort!
  rollback completed
  abort: unknown revision 'd133b886da6874fe25998d26ae1b2b8528b07c59'!
  [255]
