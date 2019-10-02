# Initial setup
  $ enable amend commitcloud infinitepush rebase snapshot
  $ setconfig extensions.treemanifest=!
  $ setconfig visibility.enabled=true
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""
  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=server
  $ setconfig infinitepushbackup.hostname=testhost
  $ setconfig snapshot.enable-sync-bundle=true

# Setup server
  $ hg init server
  $ cd server
  $ setupserver
  $ setconfig infinitepush.server=yes infinitepush.reponame=testrepo
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk
  $ touch base
  $ hg commit -Aqm base
  $ hg phase -p .
  $ BASEREV="$(hg id -i)"
  $ echo "$BASEREV"
  df4f53cec30a
  $ cd ..

# Setup clients
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg debugvisibility start
  $ hg cloud auth -t xxxxxx
  setting authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ cd ..
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ cd ..

  $ testdb() {
  > cat commitcloudservicedb | grep "$1" >> /dev/null && echo "$1" || echo "not found"
  > cat commitcloudservicedb | grep "$2" >> /dev/null && echo "$2" || echo "not found"
  > }


# Snapshot sync test plan:
# 1) Create a snapshot on each host (expected: h1 [s1], h2 [s2], s []);
# 2) Do the sync on the first host (expected: h1 [s1], h2 [s2], s [s1]);
# 3) Do the sync on the second one (expected: h1 [s1], h2 [s1, s2], s [s1, s2]);
# 3.1) Remove snapshot ext from 2nd host and do the sync (expected: nothing changes);
# 4) Remove s1 from the first host and do the sync (expected: h1 [s2], h2 [s1, s2], s [s2]);
# 5) Remove s2 from the second host and do the sync (expected: h1 [s2], h2 [], s []);
# 6) Add s1 to the first host and do the sync on the first host (expected: h1: [s1], h2 [], s[s1]).


# 1) Create a snapshot on each host (expected: h1 [s1], h2 [s2], s []);
  $ cd client1
  $ echo "snapshot1 data" > data1
  $ OID1="$(hg snapshot create --clean | head -n 1 | cut -f2 -d' ')"
  $ echo "$OID1"
  dce5266c40f04289559afbb27e0686f5bb9d9c29
  $ hg snapshot show --debug $OID1
  changeset:   1:dce5266c40f04289559afbb27e0686f5bb9d9c29
  tag:         tip
  phase:       draft
  parent:      0:df4f53cec30af1e4f669102135076fd4f9673fcc
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    4e7eb8574ed56675aa89d2b5abbced12d5688cef
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  extra:       branch=default
  extra:       snapshotmetadataid=e3bafde7a5ebc3aeac0ef6637e909554fd55e5158a78326793857b021f99a9be
  description:
  snapshot
  
  
  
  ===
  Untracked changes:
  ===
  ? data1
  @@ -0,0 +1,1 @@
  +snapshot1 data
  
  $ hg snapshot list
  dce5266c40f0 snapshot

  $ cd ../client2
  $ echo "snapshot2 data" > data2
  $ OID2="$(hg snapshot create --clean | head -n 1 | cut -f2 -d' ')"
  $ echo "$OID2"
  509860fa520fa5374e798264daef3a817c9a0169
  $ hg snapshot show --debug $OID2
  changeset:   1:509860fa520fa5374e798264daef3a817c9a0169
  tag:         tip
  phase:       draft
  parent:      0:df4f53cec30af1e4f669102135076fd4f9673fcc
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    4e7eb8574ed56675aa89d2b5abbced12d5688cef
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  extra:       branch=default
  extra:       snapshotmetadataid=17e5538a342f5ebdf88397bf30923c67f211c7490c423fc53336d67e797af6f4
  description:
  snapshot
  
  
  
  ===
  Untracked changes:
  ===
  ? data2
  @@ -0,0 +1,1 @@
  +snapshot2 data
  
  $ hg snapshot list
  509860fa520f snapshot

  $ cd ..
  $ testdb "$OID1" "$OID2"
  not found
  not found


# 2) Do the sync on the first host (expected: h1 [s1], h2 [s2], s [s1]);
  $ cd client1
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at dce5266c40f0
  remote: pushing 1 commit:
  remote:     dce5266c40f0  snapshot
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ hg snapshot list
  dce5266c40f0 snapshot

  $ cd ..
  $ testdb "$OID1" "$OID2"
  dce5266c40f04289559afbb27e0686f5bb9d9c29
  not found


# 3) Do the sync on the second one (expected: h1 [s1], h2 [s1, s2], s [s1, s2]);
  $ cd client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 509860fa520f
  remote: pushing 1 commit:
  remote:     509860fa520f  snapshot
  pulling dce5266c40f0
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets dce5266c40f0
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ hg snapshot list
  509860fa520f snapshot
  dce5266c40f0 snapshot
  $ hg snapshot show --debug "$OID1"
  changeset:   2:dce5266c40f04289559afbb27e0686f5bb9d9c29
  tag:         tip
  phase:       draft
  parent:      0:df4f53cec30af1e4f669102135076fd4f9673fcc
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    4e7eb8574ed56675aa89d2b5abbced12d5688cef
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  extra:       branch=default
  extra:       snapshotmetadataid=e3bafde7a5ebc3aeac0ef6637e909554fd55e5158a78326793857b021f99a9be
  description:
  snapshot
  
  
  
  ===
  Untracked changes:
  ===
  ? data1
  @@ -0,0 +1,1 @@
  +snapshot1 data
  

  $ cd ..
  $ testdb "$OID1" "$OID2"
  dce5266c40f04289559afbb27e0686f5bb9d9c29
  509860fa520fa5374e798264daef3a817c9a0169


# 3.1) Remove snapshot ext from 2nd host and do the sync (expected: nothing changes);
  $ cd client2
  $ setconfig extensions.snapshot=!
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ setconfig extensions.snapshot=
  $ hg snapshot list
  509860fa520f snapshot
  dce5266c40f0 snapshot

  $ cd ..
  $ testdb "$OID1" "$OID2"
  dce5266c40f04289559afbb27e0686f5bb9d9c29
  509860fa520fa5374e798264daef3a817c9a0169


# 4) Remove s1 from the first host and do the sync (expected: h1 [s2], h2 [s1, s2], s [s2]);
  $ cd client1
  $ hg snapshot hide "$OID1"
  $ hg snapshot list
  no snapshots created
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 509860fa520f
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets 509860fa520f
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ hg snapshot list
  509860fa520f snapshot

  $ hg snapshot show --debug "$OID2"
  changeset:   2:509860fa520fa5374e798264daef3a817c9a0169
  tag:         tip
  phase:       draft
  parent:      0:df4f53cec30af1e4f669102135076fd4f9673fcc
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    4e7eb8574ed56675aa89d2b5abbced12d5688cef
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  extra:       branch=default
  extra:       snapshotmetadataid=17e5538a342f5ebdf88397bf30923c67f211c7490c423fc53336d67e797af6f4
  description:
  snapshot
  
  
  
  ===
  Untracked changes:
  ===
  ? data2
  @@ -0,0 +1,1 @@
  +snapshot2 data
  

  $ cd ..
  $ testdb "$OID1" "$OID2"
  not found
  509860fa520fa5374e798264daef3a817c9a0169


# 5) Remove s2 from the second host and do the sync (expected: h1 [s2], h2 [], s []);
  $ cd client2
  $ hg snapshot hide "$OID2"
  $ hg snapshot list
  dce5266c40f0 snapshot
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ hg snapshot list
  no snapshots created

  $ cd ..
  $ testdb "$OID1" "$OID2"
  not found
  not found


# 6) Add s1 to the first host and do the sync on the first host (expected: h1: [s1], h2 [], s[s1]).
  $ cd client1
  $ hg snapshot unhide "$OID1"
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ hg snapshot list
  dce5266c40f0 snapshot

  $ cd ..
  $ testdb "$OID1" "$OID2"
  dce5266c40f04289559afbb27e0686f5bb9d9c29
  not found
