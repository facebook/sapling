#chg-compatible

#chg-compatible

#testcases nostackpush stackpush
  $ setconfig extensions.treemanifest=!
  $ setconfig experimental.evolution=
  $ . helpers-usechg.sh

  $ . "$TESTDIR/library.sh"
  $ getmysqldb
  $ createpushrebaserecordingdb

Setup

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > username = nobody <no.reply@fb.com>
  > [extensions]
  > strip =
  > EOF

#if nostackpush
  $ setconfig pushrebase.trystackpush=false
#endif
#if stackpush
  $ setconfig pushrebase.trystackpush=true
#endif

  $ commit() {
  >   hg commit -d "0 0" -A -m "$@"
  > }

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{node|short}] {bookmarks}" "$@"
  > }

Set up server repository

  $ hg init server
  $ cd server
  $ echo foo > a
  $ echo foo > b
  $ commit 'initial'
  adding a
  adding b

Set up client repository
  $ cd ..
  $ hg clone ssh://user@dummy/server client -q
  $ cd client
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase =" >> .hg/hgrc

Setup servers
  $ cd ../server
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase =" >> .hg/hgrc
  $ mkcommit 'server commit'

  $ cd ..
  $ hg clone ssh://user@dummy/server server2 -q
  $ hg clone ssh://user@dummy/server server3 -q
  $ cp server/.hg/hgrc server2/.hg/hgrc
  $ cp server/.hg/hgrc server3/.hg/hgrc

Setup pushrebase bundle recording on the first server
  $ cd server
  $ cat >> $TESTTMP/uploader.sh <<EOF
  > #! /bin/bash
  > cp \$1 $TESTTMP/bundle
  > printf handle
  > EOF
  $ chmod +x $TESTTMP/uploader.sh

  $ cat >> .hg/hgrc <<EOF
  > [pushrebase]
  > bundlepartuploadbinary=$TESTTMP/uploader.sh {filename}
  > enablerecording=True
  > recordingsqlargs=$DBHOST:$DBPORT:$DBNAME:$DBUSER:$DBPASS
  > recordingrepoid=42
  > EOF

Make a push from the client
  $ cd ../client
  $ mkcommit 'client push'
  $ hg log -r . -T 'client draft commit hash: {node}'
  client draft commit hash: 772868146114ac9fd3b573f578ee9d39d68f460e (no-eol)
  $ hg push -r . --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 changeset:
  remote:     772868146114  client push
  remote: 2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files
  $ log
  o  client push [draft:a8078509f8d1]
  |
  o  server commit [draft:bb4844f92c89]
  |
  | @  client push [draft:772868146114]
  |/
  o  initial [public:2bb9d20e471c]
  
Apply a bundle on the second server via the command line
  $ cd ../server2
  $ hg unbundle $TESTTMP/bundle
  new changesets a8078509f8d1
  $ hg log -r a8078509f8d1
  changeset:   2:a8078509f8d1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     client push
  

Apply a bundle on the third server with rewritten dates
  $ cd ../server3

  $ cat >> $TESTTMP/encode_json.py <<EOF
  > import json
  > import sys
  > start = int(sys.argv[2])
  > hashes = { h:(date + start) for date, h in enumerate(sys.argv[1].split(','))}
  > print(json.dumps(hashes))
  > EOF
  $ python $TESTTMP/encode_json.py  772868146114ac9fd3b573f578ee9d39d68f460e 3 > $TESTTMP/commitdatesfile

Try unbundle with bad commitdates file
  $ echo corrupt > $TESTTMP/corrupt
  $ hg unbundle $TESTTMP/bundle --config pushrebase.commitdatesfile=$TESTTMP/corrupt
  abort: commitdatesfile is either nonexistent or corrupted
  [255]
  $ echo "{}" > $TESTTMP/corrupt
  $ hg unbundle $TESTTMP/bundle --config pushrebase.commitdatesfile=$TESTTMP/corrupt
  abort: 772868146114ac9fd3b573f578ee9d39d68f460e not found in commitdatesfile
  [255]
  $ hg unbundle $TESTTMP/bundle --config pushrebase.commitdatesfile=$TESTTMP/nonexistent
  abort: commitdatesfile is either nonexistent or corrupted
  [255]

Now try with correct file
  $ hg unbundle $TESTTMP/bundle --config pushrebase.commitdatesfile=$TESTTMP/commitdatesfile
  new changesets d85a52e5321a
  $ hg show d85a52e5321a
  changeset:   2:d85a52e5321a
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  files:       client push
  description:
  client push
  
  
  diff -r bb4844f92c89 -r d85a52e5321a client push
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/client push	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +client push
  
  $ hg log -r d85a52e5321a  -T '{date}'
  3.00 (no-eol)



Push a stack
  $ cd ../client
  $ hg up -q default
  $ mkcommit 'stack push 1'
  $ mkcommit 'stack push 2'
  $ log
  @  stack push 2 [draft:b01ae7689fd2]
  |
  o  stack push 1 [draft:c661726b7d93]
  |
  o  client push [draft:a8078509f8d1]
  |
  o  server commit [draft:bb4844f92c89]
  |
  | o  client push [draft:772868146114]
  |/
  o  initial [public:2bb9d20e471c]
  
  $ hg log -r '.^::.' -T '{node}\n'
  c661726b7d9318b82a36fc87d368067982e6d470
  b01ae7689fd28b3b0eb2d005cad16a46084b0d42
  $ hg push -r . --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 2 changesets:
  remote:     c661726b7d93  stack push 1
  remote:     b01ae7689fd2  stack push 2

Apply stack
  $ cd ../server2
  $ python $TESTTMP/encode_json.py  c661726b7d9318b82a36fc87d368067982e6d470,b01ae7689fd28b3b0eb2d005cad16a46084b0d42  3 > $TESTTMP/commitdatesfile
  $ hg unbundle $TESTTMP/bundle --config pushrebase.commitdatesfile=$TESTTMP/commitdatesfile
  new changesets b5e2b8071144:143d91ad57b2
  $ log
  o  stack push 2 [public:143d91ad57b2]
  |
  o  stack push 1 [public:b5e2b8071144]
  |
  o  client push [public:a8078509f8d1]
  |
  @  server commit [public:bb4844f92c89]
  |
  o  initial [public:2bb9d20e471c]
  
  $ hg log -r 143d91ad57b2
  changeset:   4:143d91ad57b2
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     stack push 2
  
  $ hg log -r b5e2b8071144
  changeset:   3:b5e2b8071144
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     stack push 1
  
Create and push a commit with different timezone
  $ cd ..
  $ rm -rf server2
  $ rm -rf server3
  $ hg clone ssh://user@dummy/server server2 -q
  $ hg clone ssh://user@dummy/server server3 -q
  $ cp server/.hg/hgrc server2/.hg/hgrc
  $ cp server/.hg/hgrc server3/.hg/hgrc
  $ cd client
  $ echo newfile > newfile && hg add newfile
  $ hg commit -d "0 5" -A -m "another timezone"
  $ hg push -r . --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 changeset:
  remote:     f8e54def88a9  another timezone

  $ cd ../server2
  $ hg unbundle $TESTTMP/bundle
  new changesets f8e54def88a9

  $ cd ../server3
  $ python $TESTTMP/encode_json.py  f8e54def88a9cc429ae2077991fdc80e3f4ab5b7  0 > $TESTTMP/commitdatesfile
  $ hg unbundle $TESTTMP/bundle --config pushrebase.commitdatesfile=$TESTTMP/commitdatesfile
  new changesets f8e54def88a9
