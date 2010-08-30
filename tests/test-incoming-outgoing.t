  $ mkdir test
  $ cd test
  $ hg init
  $ for i in 0 1 2 3 4 5 6 7 8; do
  >     echo $i >> foo
  >     hg commit -A -m $i -d "1000000 0"
  > done
  adding foo
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 9 changesets, 9 total revisions
  $ hg serve -p $HGPORT -d --pid-file=hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..

  $ hg init new

http incoming

  $ hg -R new incoming http://localhost:$HGPORT/ | sed -e "s,:$HGPORT/,:\$HGPORT/,"
  comparing with http://localhost:$HGPORT/
  changeset:   0:9cb21d99fe27
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     0
  
  changeset:   1:d717f5dfad6a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  
  changeset:   2:c0d6b86da426
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2
  
  changeset:   3:dfacbd43b3fe
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     3
  
  changeset:   4:1f3a964b6022
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     4
  
  changeset:   5:c028bcc7a28a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     5
  
  changeset:   6:a0c0095f3389
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     6
  
  changeset:   7:d4be65f4e891
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     7
  
  changeset:   8:92b83e334ef8
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     8
  
  $ hg -R new incoming -r 4 http://localhost:$HGPORT/ | sed -e "s,:$HGPORT/,:\$HGPORT/,"
  comparing with http://localhost:$HGPORT/
  changeset:   0:9cb21d99fe27
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     0
  
  changeset:   1:d717f5dfad6a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  
  changeset:   2:c0d6b86da426
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2
  
  changeset:   3:dfacbd43b3fe
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     3
  
  changeset:   4:1f3a964b6022
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     4
  

local incoming

  $ hg -R new incoming test
  comparing with test
  changeset:   0:9cb21d99fe27
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     0
  
  changeset:   1:d717f5dfad6a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  
  changeset:   2:c0d6b86da426
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2
  
  changeset:   3:dfacbd43b3fe
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     3
  
  changeset:   4:1f3a964b6022
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     4
  
  changeset:   5:c028bcc7a28a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     5
  
  changeset:   6:a0c0095f3389
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     6
  
  changeset:   7:d4be65f4e891
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     7
  
  changeset:   8:92b83e334ef8
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     8
  
  $ hg -R new incoming -r 4 test
  comparing with test
  changeset:   0:9cb21d99fe27
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     0
  
  changeset:   1:d717f5dfad6a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  
  changeset:   2:c0d6b86da426
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2
  
  changeset:   3:dfacbd43b3fe
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     3
  
  changeset:   4:1f3a964b6022
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     4
  

limit to 2 changesets

  $ hg -R new incoming -l 2 test
  comparing with test
  changeset:   0:9cb21d99fe27
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     0
  
  changeset:   1:d717f5dfad6a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  

limit to 2 changesets, test with -p --git

  $ hg -R new incoming -l 2 -p --git test
  comparing with test
  changeset:   0:9cb21d99fe27
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     0
  
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,1 @@
  +0
  
  changeset:   1:d717f5dfad6a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -1,1 +1,2 @@
   0
  +1
  

test with --bundle

  $ hg -R new incoming --bundle test.hg http://localhost:$HGPORT/ | sed -e "s,:$HGPORT/,:\$HGPORT/,"
  comparing with http://localhost:$HGPORT/
  changeset:   0:9cb21d99fe27
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     0
  
  changeset:   1:d717f5dfad6a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  
  changeset:   2:c0d6b86da426
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2
  
  changeset:   3:dfacbd43b3fe
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     3
  
  changeset:   4:1f3a964b6022
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     4
  
  changeset:   5:c028bcc7a28a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     5
  
  changeset:   6:a0c0095f3389
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     6
  
  changeset:   7:d4be65f4e891
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     7
  
  changeset:   8:92b83e334ef8
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     8
  
  $ hg -R new incoming --bundle test2.hg test
  comparing with test
  changeset:   0:9cb21d99fe27
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     0
  
  changeset:   1:d717f5dfad6a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  
  changeset:   2:c0d6b86da426
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2
  
  changeset:   3:dfacbd43b3fe
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     3
  
  changeset:   4:1f3a964b6022
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     4
  
  changeset:   5:c028bcc7a28a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     5
  
  changeset:   6:a0c0095f3389
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     6
  
  changeset:   7:d4be65f4e891
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     7
  
  changeset:   8:92b83e334ef8
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     8
  


test the resulting bundles

  $ hg init temp
  $ hg init temp2
  $ hg -R temp unbundle test.hg
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 9 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg -R temp2 unbundle test2.hg
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 9 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg -R temp tip
  changeset:   8:92b83e334ef8
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     8
  
  $ hg -R temp2 tip
  changeset:   8:92b83e334ef8
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     8
  

  $ rm -r temp temp2 new

test outgoing

  $ hg clone test test-dev
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd test-dev
  $ for i in 9 10 11 12 13; do
  >     echo $i >> foo
  >     hg commit -A -m $i -d "1000000 0"
  > done
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 14 changesets, 14 total revisions
  $ cd ..
  $ hg -R test-dev outgoing test
  comparing with test
  searching for changes
  changeset:   9:3741c3ad1096
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     9
  
  changeset:   10:de4143c8d9a5
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     10
  
  changeset:   11:0e1c188b9a7a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     11
  
  changeset:   12:251354d0fdd3
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     12
  
  changeset:   13:bdaadd969642
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     13
  

limit to 3 changesets

  $ hg -R test-dev outgoing -l 3 test
  comparing with test
  searching for changes
  changeset:   9:3741c3ad1096
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     9
  
  changeset:   10:de4143c8d9a5
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     10
  
  changeset:   11:0e1c188b9a7a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     11
  
  $ hg -R test-dev outgoing http://localhost:$HGPORT/ | sed -e "s,:$HGPORT/,:\$HGPORT/,"
  comparing with http://localhost:$HGPORT/
  searching for changes
  changeset:   9:3741c3ad1096
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     9
  
  changeset:   10:de4143c8d9a5
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     10
  
  changeset:   11:0e1c188b9a7a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     11
  
  changeset:   12:251354d0fdd3
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     12
  
  changeset:   13:bdaadd969642
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     13
  
  $ hg -R test-dev outgoing -r 11 http://localhost:$HGPORT/ | sed -e "s,:$HGPORT/,:\$HGPORT/,"
  comparing with http://localhost:$HGPORT/
  searching for changes
  changeset:   9:3741c3ad1096
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     9
  
  changeset:   10:de4143c8d9a5
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     10
  
  changeset:   11:0e1c188b9a7a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     11
  
