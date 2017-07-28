  $ . $TESTDIR/library.sh
  $ hg init repo
  $ cd repo
  $ touch a
  $ hg add a
  $ hg ci -ma
  $ hg log
  changeset:   0:3903775176ed
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ cd ..
  $ mkdir $TESTTMP/blobrepo
  $ blobimport --blobstore files repo $TESTTMP/blobrepo
  * INFO 0: changeset 3903775176ed42b1458a6281db4a0ccf4d9f287a (glob)
  *INFO head 3903775176ed42b1458a6281db4a0ccf4d9f287a (glob)

Temporary hack because blobimport doesn't import bookmarks yet
  $ mkdir $TESTTMP/blobrepo/bookmarks
  $ edenserver --addr 127.0.0.1:3000 --blobrepo-folder $TESTTMP/blobrepo --reponame repo

Temporary hack to make sure server is ready
  $ sleep 1

Curl and debugdata output should match
  $ curl http://localhost:3000/repo/cs/3903775176ed42b1458a6281db4a0ccf4d9f287a/roottreemanifestid 2> /dev/null
  8515d4bfda768e04af4c13a69a72e28c7effbea7 (no-eol)
  $ cd repo
  $ hg debugdata -c 3903775176ed42b1458a6281db4a0ccf4d9f287a | head -n 1
  8515d4bfda768e04af4c13a69a72e28c7effbea7

  $ curl http://localhost:3000/repo/cs/hash/roottreemanifestid 2> /dev/null
  invalid sha-1 input (no-eol)
  $ curl http://localhost:3000/badrepo/cs/3903775176ed42b1458a6281db4a0ccf4d9f287a/roottreemanifestid 2> /dev/null
  unknown repo (no-eol)
  $ curl http://localhost:3000/repo/BADURL/3903775176ed42b1458a6281db4a0ccf4d9f287a/roottreemanifestid 2> /dev/null
  malformed url: expected /REPONAME/cs/HASH/roottreemanifestid (no-eol)

Make sure that there were no errors server side
  $ cat $TESTTMP/edenserver.out
