#chg-compatible
  $ setconfig experimental.allowfilepeer=True
  $ setconfig devel.segmented-changelog-rev-compat=true

#require jq
  $ configure mutation-norecord dummyssh
  $ enable amend commitcloud infinitepush rebase remotenames share smartlog

  $ setconfig infinitepush.branchpattern="re:scratch/.*"
  $ setconfig commitcloud.hostname=testhost commitcloud.max_sync_age=14

  $ setconfig remotefilelog.reponame=server

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  > [infinitepush]
  > server = yes
  > indextype = disk
  > storetype = disk
  > reponame = testrepo
  > [treemanifest]
  > server = True
  > EOF
  $ touch base
  $ hg commit -Aqm base
  $ hg debugmakepublic .
  $ hg bookmark master
  $ cd ..

Make shared part of config
  $ cat >> shared.rc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > token_enforced = False
  > education_page = https://someurl.com/wiki/CommitCloud
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

Connect the first client
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

Make some stacks with various dates.  We will use Feb 1990 for these tests.

- Bookmark for public commit
  $ hg up -q 'desc(base)'
  $ hg bookmark 'mytag'

- Old stack
  $ hg up -q 'desc(base)'
  $ touch oldstack-feb1
  $ hg commit -Aqm oldstack-feb1 --config devel.default-date="1990-02-01T00:00Z"
  $ hg book -i oldbook
  $ touch oldstack-feb4
  $ hg commit -Aqm oldstack-feb4 --config devel.default-date="1990-02-04T12:00Z"

- Middle stack
  $ hg up -q 'desc(base)'
  $ touch midstack-feb7
  $ hg commit -Aqm midstack-feb7 --config devel.default-date="1990-02-07T00:00Z"
  $ hg book -i midbook
  $ touch midstack-feb9
  $ hg commit -Aqm midstack-feb9 --config devel.default-date="1990-02-09T12:00Z"

- New stack
  $ hg up -q 'desc(base)'
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
  backing up stack rooted at 1c1b7955142c
  backing up stack rooted at 56a352317b67
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     1f9ebd6d1390  oldstack-feb1
  remote:     d16408588b2d  oldstack-feb4
  remote: pushing 2 commits:
  remote:     1c1b7955142c  midstack-feb7
  remote:     d133b886da68  midstack-feb9
  remote: pushing 2 commits:
  remote:     56a352317b67  newstack-feb13
  remote:     7f958333fe84  newstack-feb15

  $ tglogp
  @  7f958333fe84 draft 'newstack-feb15'
  │
  o  56a352317b67 draft 'newstack-feb13' newbook
  │
  │ o  d133b886da68 draft 'midstack-feb9'
  │ │
  │ o  1c1b7955142c draft 'midstack-feb7' midbook
  ├─╯
  │ o  d16408588b2d draft 'oldstack-feb4'
  │ │
  │ o  1f9ebd6d1390 draft 'oldstack-feb1' oldbook
  ├─╯
  o  df4f53cec30a public 'base' mytag
  

  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 2
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      mytag => df4f53cec30af1e4f669102135076fd4f9673fcc
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
  omitting 1 head that is older than 14 days:
    d16408588b2d from Sun Feb 04 12:00:00 1990 +0000
  pulling d133b886da68 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  pulling 7f958333fe84 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  1f9ebd6d1390 not found, omitting oldbook bookmark
  df4f53cec30a is older than 14 days, omitting mytag bookmark
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglogp
  o  7f958333fe84 draft 'newstack-feb15'
  │
  o  56a352317b67 draft 'newstack-feb13' newbook
  │
  │ o  d133b886da68 draft 'midstack-feb9'
  │ │
  │ o  1c1b7955142c draft 'midstack-feb7' midbook
  ├─╯
  @  df4f53cec30a public 'base'
  

Create a new commit
  $ hg up 'desc(base)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch client2-file1
  $ hg commit -Aqm client2-feb28 --config devel.default-date="1990-02-28T01:00Z"
  $ (cat $TESTTMP/nodedata ; hg log -r . -Tjson) | jq -s add > $TESTTMP/nodedata.new
  $ mv $TESTTMP/nodedata.new $TESTTMP/nodedata
  $ hgfakedate 1990-02-28T01:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at ff52de2f760c
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 1 commit:
  remote:     ff52de2f760c  client2-feb28

Sync these commits to the first client - it has everything
  $ cd ../client1
  $ hgfakedate 1990-02-28T01:02Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling ff52de2f760c from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  ff52de2f760c draft 'client2-feb28'
  │
  │ @  7f958333fe84 draft 'newstack-feb15'
  │ │
  │ o  56a352317b67 draft 'newstack-feb13' newbook
  ├─╯
  │ o  d133b886da68 draft 'midstack-feb9'
  │ │
  │ o  1c1b7955142c draft 'midstack-feb7' midbook
  ├─╯
  │ o  d16408588b2d draft 'oldstack-feb4'
  │ │
  │ o  1f9ebd6d1390 draft 'oldstack-feb1' oldbook
  ├─╯
  o  df4f53cec30a public 'base' mytag
  

Second client can still sync
  $ cd ../client2
  $ hgfakedate 1990-02-28T01:22Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  @  ff52de2f760c draft 'client2-feb28'
  │
  │ o  7f958333fe84 draft 'newstack-feb15'
  │ │
  │ o  56a352317b67 draft 'newstack-feb13' newbook
  ├─╯
  │ o  d133b886da68 draft 'midstack-feb9'
  │ │
  │ o  1c1b7955142c draft 'midstack-feb7' midbook
  ├─╯
  o  df4f53cec30a public 'base'
  

  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 3
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      mytag => df4f53cec30af1e4f669102135076fd4f9673fcc
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
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 3 commits:
  remote:     56a352317b67  newstack-feb13
  remote:     7f958333fe84  newstack-feb15
  remote:     46f8775ee5d4  newstack-feb28

  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 4
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      mytag => df4f53cec30af1e4f669102135076fd4f9673fcc
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
  omitting 1 head that is older than 14 days:
    d16408588b2d from Sun Feb 04 12:00:00 1990 +0000
  pulling 46f8775ee5d4 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  46f8775ee5d4 draft 'newstack-feb28'
  │
  │ @  ff52de2f760c draft 'client2-feb28'
  │ │
  o │  7f958333fe84 draft 'newstack-feb15'
  │ │
  o │  56a352317b67 draft 'newstack-feb13' newbook
  ├─╯
  │ o  d133b886da68 draft 'midstack-feb9'
  │ │
  │ o  1c1b7955142c draft 'midstack-feb7' midbook
  ├─╯
  o  df4f53cec30a public 'base'
  

  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 4
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      mytag => df4f53cec30af1e4f669102135076fd4f9673fcc
      newbook => 56a352317b67ae3d5abd5f6c71ec0df3aa98fe97
      oldbook => 1f9ebd6d1390ebc603e401171eda0c444a0f8754
  heads:
      d16408588b2d047410f99c45e425bf97923e28f2
      d133b886da6874fe25998d26ae1b2b8528b07c59
      ff52de2f760c67fa6f89273ca7f770396a3c81c4
      46f8775ee5d479eed945b5186929bd046f116176

First client add a new commit to the old stack
  $ cd ../client1
  $ hg up d16408588b2d047410f99c45e425bf97923e28f2
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved

  $ touch oldstack-mar4
  $ hg commit -Aqm oldstack-mar4 --config devel.default-date="1990-03-04T03:00Z"
  $ (cat $TESTTMP/nodedata ; hg log -r . -Tjson) | jq -s add > $TESTTMP/nodedata.new
  $ mv $TESTTMP/nodedata.new $TESTTMP/nodedata
  $ hgfakedate 1990-03-04T03:02Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 1f9ebd6d1390
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 3 commits:
  remote:     1f9ebd6d1390  oldstack-feb1
  remote:     d16408588b2d  oldstack-feb4
  remote:     2b8dce7bd745  oldstack-mar4

  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 5
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      mytag => df4f53cec30af1e4f669102135076fd4f9673fcc
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
  pulling 2b8dce7bd745 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  2b8dce7bd745 draft 'oldstack-mar4'
  │
  o  d16408588b2d draft 'oldstack-feb4'
  │
  o  1f9ebd6d1390 draft 'oldstack-feb1' oldbook
  │
  │ o  46f8775ee5d4 draft 'newstack-feb28'
  │ │
  │ │ @  ff52de2f760c draft 'client2-feb28'
  ├───╯
  │ o  7f958333fe84 draft 'newstack-feb15'
  │ │
  │ o  56a352317b67 draft 'newstack-feb13' newbook
  ├─╯
  │ o  d133b886da68 draft 'midstack-feb9'
  │ │
  │ o  1c1b7955142c draft 'midstack-feb7' midbook
  ├─╯
  o  df4f53cec30a public 'base'
  
  $ python $TESTTMP/dumpcommitcloudmetadata.py
  version: 5
  bookmarks:
      midbook => 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8
      mytag => df4f53cec30af1e4f669102135076fd4f9673fcc
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
  omitting 1 head that is older than 14 days:
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
  pulling ff52de2f760c 46f8775ee5d4 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  pulling 2b8dce7bd745 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  1c1b7955142c not found, omitting midbook bookmark
  df4f53cec30a is older than 14 days, omitting mytag bookmark
  commitcloud: commits synchronized
  finished in * (glob)

  $ hgfakedate 1990-03-05T12:00Z smartlog -T '{node|short} {desc} {bookmarks}' --config infinitepushbackup.autobackup=true
  o  2b8dce7bd745 oldstack-mar4
  │
  o  d16408588b2d oldstack-feb4
  │
  o  1f9ebd6d1390 oldstack-feb1 oldbook
  │
  │ o  46f8775ee5d4 newstack-feb28
  │ │
  │ o  7f958333fe84 newstack-feb15
  │ │
  │ o  56a352317b67 newstack-feb13 newbook
  ├─╯
  │ o  ff52de2f760c client2-feb28
  ├─╯
  @  df4f53cec30a base
  
  hint[commitcloud-old-commits]: some older commits or bookmarks have not been synced to this repo
  (run 'hg cloud sl' to see all of the commits in your workspace)
  (run 'hg pull -r HASH' to fetch commits by hash)
  (run 'hg cloud sync --full' to fetch everything - this may be slow)
  hint[hint-ack]: use 'hg hint --ack commitcloud-old-commits' to silence these hints

Move one of these bookmarks in the first client.

  $ cd ../client1
  $ hg book -f -r d133b886da6874fe25998d26ae1b2b8528b07c59 oldbook
  $ hgfakedate 1990-03-05T12:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

Do a sync in the new client - the bookmark is left where it was

  $ cd ../client3
  $ hgfakedate 1990-03-05T12:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting 1 head that is older than 14 days:
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  2b8dce7bd745 draft 'oldstack-mar4'
  │
  o  d16408588b2d draft 'oldstack-feb4'
  │
  o  1f9ebd6d1390 draft 'oldstack-feb1'
  │
  │ o  ff52de2f760c draft 'client2-feb28'
  ├─╯
  │ o  46f8775ee5d4 draft 'newstack-feb28'
  │ │
  │ o  7f958333fe84 draft 'newstack-feb15'
  │ │
  │ o  56a352317b67 draft 'newstack-feb13' newbook
  ├─╯
  @  df4f53cec30a public 'base'
  
Move the bookmark locally - this still gets synced ok.

  $ hg book -f -r 46f8775ee5d479eed945b5186929bd046f116176 oldbook
  $ tglogp
  o  2b8dce7bd745 draft 'oldstack-mar4'
  │
  o  d16408588b2d draft 'oldstack-feb4'
  │
  o  1f9ebd6d1390 draft 'oldstack-feb1'
  │
  │ o  ff52de2f760c draft 'client2-feb28'
  ├─╯
  │ o  46f8775ee5d4 draft 'newstack-feb28' oldbook
  │ │
  │ o  7f958333fe84 draft 'newstack-feb15'
  │ │
  │ o  56a352317b67 draft 'newstack-feb13' newbook
  ├─╯
  @  df4f53cec30a public 'base'
  
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
  @  2b8dce7bd745 draft 'oldstack-mar4'
  │
  │ o  46f8775ee5d4 draft 'newstack-feb28' oldbook
  │ │
  │ │ o  ff52de2f760c draft 'client2-feb28'
  │ │ │
  │ o │  7f958333fe84 draft 'newstack-feb15'
  │ │ │
  │ o │  56a352317b67 draft 'newstack-feb13' newbook
  │ ├─╯
  │ │ o  d133b886da68 draft 'midstack-feb9'
  │ │ │
  │ │ o  1c1b7955142c draft 'midstack-feb7' midbook
  │ ├─╯
  o │  d16408588b2d draft 'oldstack-feb4'
  │ │
  o │  1f9ebd6d1390 draft 'oldstack-feb1'
  ├─╯
  o  df4f53cec30a public 'base' mytag
  

A full sync pulls the old commits in
  $ cd ../client3
  $ hgfakedate 1990-03-05T12:01Z cloud sync --full
  commitcloud: latest 2 years of commits will be attempted to synchronize first
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling d133b886da68 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: latest 2 years of commits synchronized
  commitcloud: latest 3 years of commits will be attempted to synchronize first
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: latest 3 years of commits synchronized
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglogp
  o  d133b886da68 draft 'midstack-feb9'
  │
  o  1c1b7955142c draft 'midstack-feb7' midbook
  │
  │ o  2b8dce7bd745 draft 'oldstack-mar4'
  │ │
  │ o  d16408588b2d draft 'oldstack-feb4'
  │ │
  │ o  1f9ebd6d1390 draft 'oldstack-feb1'
  ├─╯
  │ o  ff52de2f760c draft 'client2-feb28'
  ├─╯
  │ o  46f8775ee5d4 draft 'newstack-feb28' oldbook
  │ │
  │ o  7f958333fe84 draft 'newstack-feb15'
  │ │
  │ o  56a352317b67 draft 'newstack-feb13' newbook
  ├─╯
  @  df4f53cec30a public 'base' mytag
  
Create a new client that isn't connected yet
  $ cd ..
  $ hg clone ssh://user@dummy/server client4 -q
  $ cat shared.rc >> client4/.hg/hgrc

A part sync omitting everything
  $ cd ./client4
  $ hgfakedate 1990-04-01T12:01Z cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting 4 heads that are older than 14 days:
    2b8dce7bd745 from Sun Mar 04 03:00:00 1990 +0000
    46f8775ee5d4 from Wed Feb 28 02:00:00 1990 +0000
    ff52de2f760c from Wed Feb 28 01:00:00 1990 +0000
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
  * not found, omitting * bookmark (glob)
  * not found, omitting * bookmark (glob)
  df4f53cec30a is older than 14 days, omitting mytag bookmark
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  @  df4f53cec30a public 'base'
  
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
  omitting 4 heads that are older than 14 days:
    2b8dce7bd745 from Sun Mar 04 03:00:00 1990 +0000
    46f8775ee5d4 from Wed Feb 28 02:00:00 1990 +0000
    ff52de2f760c from Wed Feb 28 01:00:00 1990 +0000
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
  commitcloud: commits synchronized
  finished in * (glob)

Pull in some of the commits by setting max age manually
  $ hgfakedate 1990-04-01T12:01Z cloud sync --config commitcloud.max_sync_age=30
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting 3 heads that are older than 30 days:
    46f8775ee5d4 from Wed Feb 28 02:00:00 1990 +0000
    ff52de2f760c from Wed Feb 28 01:00:00 1990 +0000
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
  pulling 2b8dce7bd745 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglogp
  o  2b8dce7bd745 draft 'oldstack-mar4'
  │
  o  d16408588b2d draft 'oldstack-feb4'
  │
  o  1f9ebd6d1390 draft 'oldstack-feb1'
  │
  @  df4f53cec30a public 'base'
  

Create a bookmark with the same name as an omitted bookmark
  $ hg book -r tip midbook
  $ hgfakedate 1990-04-01T12:01Z cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  omitting 3 heads that are older than 14 days:
    46f8775ee5d4 from Wed Feb 28 02:00:00 1990 +0000
    ff52de2f760c from Wed Feb 28 01:00:00 1990 +0000
    d133b886da68 from Fri Feb 09 12:00:00 1990 +0000
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
  o  d133b886da68 draft 'midstack-feb9'
  │
  o  1c1b7955142c draft 'midstack-feb7'
  │
  │ o  2b8dce7bd745 draft 'oldstack-mar4' midbook
  │ │
  │ o  d16408588b2d draft 'oldstack-feb4'
  │ │
  │ o  1f9ebd6d1390 draft 'oldstack-feb1'
  ├─╯
  │ o  ff52de2f760c draft 'client2-feb28'
  ├─╯
  │ o  46f8775ee5d4 draft 'newstack-feb28'
  │ │
  │ o  7f958333fe84 draft 'newstack-feb15'
  │ │
  │ o  56a352317b67 draft 'newstack-feb13'
  ├─╯
  @  df4f53cec30a public 'base' mytag
  

In client1 (which hasn't synced yet), make the midbook commit obsolete.
  $ cd ../client1
  $ hg up -q 2b8dce7bd745
  $ hg amend -m "oldstack-mar4 amended"

Attempt to sync.  The midbook bookmark should make it visible again.
  $ hg cloud sync -q
  $ tglog
  @  2ace67ee4791 'oldstack-mar4 amended'
  │
  │ x  2b8dce7bd745 'oldstack-mar4' midbook
  ├─╯
  │ o  46f8775ee5d4 'newstack-feb28'
  │ │
  │ │ o  ff52de2f760c 'client2-feb28'
  │ │ │
  │ o │  7f958333fe84 'newstack-feb15'
  │ │ │
  │ o │  56a352317b67 'newstack-feb13'
  │ ├─╯
  │ │ o  d133b886da68 'midstack-feb9'
  │ │ │
  │ │ o  1c1b7955142c 'midstack-feb7'
  │ ├─╯
  o │  d16408588b2d 'oldstack-feb4'
  │ │
  o │  1f9ebd6d1390 'oldstack-feb1'
  ├─╯
  o  df4f53cec30a 'base' mytag
  
Sync in client2.  It should match.
  $ cd ../client2
  $ hg cloud sync -q
  $ tglog
  o  2ace67ee4791 'oldstack-mar4 amended'
  │
  │ x  2b8dce7bd745 'oldstack-mar4' midbook
  ├─╯
  o  d16408588b2d 'oldstack-feb4'
  │
  o  1f9ebd6d1390 'oldstack-feb1'
  │
  │ o  46f8775ee5d4 'newstack-feb28'
  │ │
  │ │ @  ff52de2f760c 'client2-feb28'
  ├───╯
  │ o  7f958333fe84 'newstack-feb15'
  │ │
  │ o  56a352317b67 'newstack-feb13'
  ├─╯
  │ o  d133b886da68 'midstack-feb9'
  │ │
  │ o  1c1b7955142c 'midstack-feb7'
  ├─╯
  o  df4f53cec30a 'base'
  
Hide some uninteresting commits and sync everywhere
  $ hg hide -r 1c1b7955142cd8a3beec705c9cca9d775ecb0fa8:: -r 56a352317b67ae3d5abd5f6c71ec0df3aa98fe97:: -r ff52de2f760c67fa6f89273ca7f770396a3c81c4::
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
  $ hg debugmakepublic .

Pull this into client1
  $ cd ../client1
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes

Move midbook to the public commit.
  $ hg book -fr 'desc(public1)' midbook
  $ hg cloud sync -q

Sync in client 2.  It doesn't have the new destination of midbook, so should omit it.

  $ cd ../client2
  $ hg cloud sync -q
  f770b7f72fa5 not found, omitting midbook bookmark
  $ tglogp
  o  2ace67ee4791 draft 'oldstack-mar4 amended'
  │
  o  d16408588b2d draft 'oldstack-feb4'
  │
  o  1f9ebd6d1390 draft 'oldstack-feb1'
  │
  @  df4f53cec30a public 'base'
  
  $ cd ../client1
  $ hg cloud sync -q
  $ tglogp
  o  f770b7f72fa5 public 'public1' midbook
  │
  │ @  2ace67ee4791 draft 'oldstack-mar4 amended'
  │ │
  │ o  d16408588b2d draft 'oldstack-feb4'
  │ │
  │ o  1f9ebd6d1390 draft 'oldstack-feb1'
  ├─╯
  o  df4f53cec30a public 'base' mytag
  
Sync in client 4.  Some of the omitted heads in this client have been removed
from the cloud workspace, but the sync should still work.

  $ cd ../client4
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 2ace67ee4791 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  f770b7f72fa5 not found, omitting midbook bookmark
  commitcloud: commits synchronized
  finished in 0.00 sec
