#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ configure dummyssh mutation-norecord
  $ enable amend arcdiff commitcloud infinitepush rebase remotenames share
  $ setconfig extensions.arcconfig="$TESTDIR/../edenscm/ext/extlib/phabricator/arcconfig.py"
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

Make the third clone of the server
  $ hg clone ssh://user@dummy/server client3 -q
  $ cd client3
  $ cat ../shared.rc >> .hg/hgrc
  $ hg cloud join -q

  $ cd ..

Test for `hg diff --since-last-submit`

  $ cd client1
  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "oauth" : "garbage_cert"}}}' > .arcconfig

  $ cd ..

  $ cd client2
  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "oauth" : "garbage_cert"}}}' > .arcconfig

  $ cd ..

  $ cd client3
  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "oauth" : "garbage_cert"}}}' > .arcconfig

  $ cd ..

  $ cd client1

  $ echo "Hello feature2" > feature2.body.txt
  $ hg add feature2.body.txt

  $ hg ci -Aqm 'Differential Revision: https://phabricator.fb.com/D1'
  $ hg log -r '.' -T '{node}'
  162e0a8b5732f1fa168b0a6d8cf9809053ae272a (no-eol)
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 162e0a8b5732
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 1 commit:
  remote:     162e0a8b5732  Differential Revision: https://phabricator.fb.com/

  $ cat > $TESTTMP/mockduit << 'EOF'
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Needs Review",
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"162e0a8b5732f1fa168b0a6d8cf9809053ae272a\"}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 1},
  >   "is_landing": false,
  >   "land_job_status": "NO_LAND_RUNNING",
  >   "needs_final_review_status": "NOT_NEEDED",
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF

  $ echo "Hello feature2 update" > feature2.body.txt
  $ hg amend

  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 1166f984c176
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 1 commit:
  remote:     1166f984c176  Differential Revision: https://phabricator.fb.com/

  $ cd ..

  $ cd client2

  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 1166f984c176 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)

  $ hg up 1166f984c176
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-submit --config extensions.commitcloud=!
  pulling '162e0a8b5732f1fa168b0a6d8cf9809053ae272a' from 'ssh://user@dummy/server'
  diff -r 162e0a8b5732 -r 1166f984c176 feature2.body.txt
  --- a/feature2.body.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/feature2.body.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -Hello feature2
  +Hello feature2 update

  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-submit
  diff -r 162e0a8b5732 -r 1166f984c176 feature2.body.txt
  --- a/feature2.body.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/feature2.body.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -Hello feature2
  +Hello feature2 update

  $ cd ..

  $ cd client3

  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 1166f984c176 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)

  $ hg up 1166f984c176
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -r 'lastsubmitted(.)' -T '{node} {desc}'  --config extensions.commitcloud=!
  pulling '162e0a8b5732f1fa168b0a6d8cf9809053ae272a' from 'ssh://user@dummy/server'
  162e0a8b5732f1fa168b0a6d8cf9809053ae272a Differential Revision: https://phabricator.fb.com/D1 (no-eol)

  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log --hidden -r 'lastsubmitted(.)' -T '{node} {desc}'
  162e0a8b5732f1fa168b0a6d8cf9809053ae272a Differential Revision: https://phabricator.fb.com/D1 (no-eol)
