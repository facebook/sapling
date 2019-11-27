  $ . helpers-usechg.sh
  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > arcconfig=$TESTDIR/../edenscm/hgext/extlib/phabricator/arcconfig.py
  > smartlog=
  > pullcreatemarkers=
  > phabstatus=
  > [infinitepushbackup]
  > createlandedasmarkers=True
  > hostname=testhost
  > logdir=$TESTTMP/logs
  > [experimental]
  > evolution= createmarkers
  > EOF

  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    url="https://phabricator.fb.com"
  >    if [ -n "$3" ]; then
  >      url="$3"
  >    fi
  >    [ -z "$2" ] || echo "Differential Revision: $url/D$2" >> msg
  >    hg ci -l msg
  > }

  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "cert" : "garbage_cert"}}}' > .arcconfig

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Set up server repository
  $ cd repo
  $ mkcommit initial
  $ mkcommit secondcommit
  $ hg book master
  $ cd ..

Set up clients repository

  $ hg clone ssh://user@dummy/repo client -q
  $ hg clone ssh://user@dummy/repo otherclient -q

Add two commits, one "pushed" to differential
  $ cd otherclient
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit b 123
  $ mkcommit c

  $ cd ..

Add commit which mimics previous differential one merged to master
  $ cd client
  $ hg up master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master)
  $ mkcommit b 123
  $ hg push --to master
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  updating bookmark master

  $ cd ..

Push all pulled commit to backup
  $ cd otherclient
  $ hg pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files
  updating bookmark master
  obsoleted 1 changesets
  new changesets 948715751816
  $ hg cloud backup
  backing up stack rooted at 9b3ead1d8005
  remote: pushing 2 commits:
  remote:     9b3ead1d8005  add b
  remote:     3969cd9723d1  add c
  commitcloud: backed up 2 commits

  $ cd ..

Clone fresh repo and try to restore from backup
  $ hg clone ssh://user@dummy/repo frombackup -q
  $ cd frombackup
  $ hg sl --all
  @  changeset:   2:948715751816
  |  bookmark:    master
  ~  tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add b
   (re)
  note: background backup is currently disabled so your commits are not being backed up.

  $ NOW=`date +%s`
  $ cat > $TESTTMP/mockduit << EOF
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 123,
  >   "diff_status_name": "Closed",
  >   "is_landing": false,
  >   "created_time": 0,
  >   "updated_time": ${NOW}
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg cloud restorebackup
  restoring backup for test from $TESTTMP/otherclient on testhost
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files
  new changesets 3969cd9723d1
  $ hg sl --all
  @  changeset:   2:948715751816
  :  bookmark:    master
  :  user:        test
  :  date:        Thu Jan 01 00:00:00 1970 +0000
  :  summary:     add b
  :
  : o  changeset:   4:3969cd9723d1
  : |  tag:         tip
  : |  user:        test
  : |  date:        Thu Jan 01 00:00:00 1970 +0000
  : |  instability: orphan
  : |  summary:     add c
  : |
  : x  changeset:   3:9b3ead1d8005
  :/   parent:      0:c255e4a1ae9d
  :    user:        test
  :    date:        Thu Jan 01 00:00:00 1970 +0000
  :    obsolete:    pruned
  :    summary:     add b
  :
  o  changeset:   0:c255e4a1ae9d
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add initial
   (re)
  note: background backup is currently disabled so your commits are not being backed up.
  $ hg debugobsolete
  9b3ead1d8005d305582e9d72eb8a4c8959873249 0 {c255e4a1ae9dd17d77787816cff012162a122798} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

  $ cd ..

Test createlandedasmarkers option disabled
  $ rm -r frombackup
  $ sed s/createlandedasmarkers=True// $HGRCPATH > ${HGRCPATH}.bak
  $ mv ${HGRCPATH}.bak $HGRCPATH
  $ hg clone ssh://user@dummy/repo frombackup -q
  $ cd frombackup

  $ NOW=`date +%s`
  $ cat > $TESTTMP/mockduit << EOF
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 123,
  >   "diff_status_name": "Closed",
  >   "created_time": 0,
  >   "updated_time": ${NOW}
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg cloud restorebackup
  restoring backup for test from $TESTTMP/otherclient on testhost
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files
  new changesets 9b3ead1d8005:3969cd9723d1
  $ hg sl --all
  @  changeset:   2:948715751816
  :  bookmark:    master
  :  user:        test
  :  date:        Thu Jan 01 00:00:00 1970 +0000
  :  summary:     add b
  :
  : o  changeset:   4:3969cd9723d1
  : |  tag:         tip
  : |  user:        test
  : |  date:        Thu Jan 01 00:00:00 1970 +0000
  : |  summary:     add c
  : |
  : o  changeset:   3:9b3ead1d8005
  :/   parent:      0:c255e4a1ae9d
  :    user:        test
  :    date:        Thu Jan 01 00:00:00 1970 +0000
  :    summary:     add b
  :
  o  changeset:   0:c255e4a1ae9d
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add initial
   (re)
  note: background backup is currently disabled so your commits are not being backed up.
  $ hg debugobsolete
