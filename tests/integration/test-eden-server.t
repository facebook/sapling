  $ . $TESTDIR/library.sh
  $ hg init repo
  $ cd repo
  $ touch a
  $ hg add a
  $ hg ci -ma
  $ echo 1 > b
  $ echo 2 > c
  $ hg add b c
  $ hg ci -mb
  $ hg log
  changeset:   1:4dabaf45f54a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ cd ..
  $ mkdir $TESTTMP/blobrepo
  $ blobimport --blobstore files repo $TESTTMP/blobrepo
  * INFO 0: changeset 3903775176ed42b1458a6281db4a0ccf4d9f287a (glob)
  * INFO 1: changeset 4dabaf45f54add88ca2797dfdeb00a7d55144243 (glob)
  * INFO head 4dabaf45f54add88ca2797dfdeb00a7d55144243 (glob)

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
  $ hg debugdata -m 8515d4bfda768e04af4c13a69a72e28c7effbea7
  a\x00b80de5d138758541c5f05265ad144ab9fa86d1db (esc)

  $ curl http://localhost:3000/repo/treenode/8515d4bfda768e04af4c13a69a72e28c7effbea7/ 2> /dev/null
  [{"hash":"b80de5d138758541c5f05265ad144ab9fa86d1db","path":"a","size":0,"type":"File"}] (no-eol)

Empty file
  $ curl http://localhost:3000/repo/blob/b80de5d138758541c5f05265ad144ab9fa86d1db/ 2> /dev/null

  $ curl http://localhost:3000/repo/cs/4dabaf45f54add88ca2797dfdeb00a7d55144243/roottreemanifestid 2> /dev/null
  b47dc781a873595c796b01e2ed5829e3fed2c887 (no-eol)
  $ curl http://localhost:3000/repo/treenode/b47dc781a873595c796b01e2ed5829e3fed2c887/ 2> /dev/null
  [{"hash":"b80de5d138758541c5f05265ad144ab9fa86d1db","path":"a","size":0,"type":"File"},{"hash":"b8e02f6433738021a065f94175c7cd23db5f05be","path":"b","size":2,"type":"File"},{"hash":"5d9299349fc01ddd25d0070d149b124d8f10411e","path":"c","size":2,"type":"File"}] (no-eol)
  $ curl http://localhost:3000/repo/blob/5d9299349fc01ddd25d0070d149b124d8f10411e/ 2> /dev/null
  2

Send incorrect requests
  $ curl http://localhost:3000/repo/cs/hash/roottreemanifestid 2> /dev/null
  invalid sha-1 input: need at least 40 hex digits (no-eol)
  $ curl http://localhost:3000/badrepo/cs/3903775176ed42b1458a6281db4a0ccf4d9f287a/roottreemanifestid 2> /dev/null
  unknown repo (no-eol)
  $ curl http://localhost:3000/badrepo/treenode/8515d4bfda768e04af4c13a69a72e28c7effbea7/ 2> /dev/null
  unknown repo (no-eol)
  $ curl http://localhost:3000/repo/BADURL/3903775176ed42b1458a6281db4a0ccf4d9f287a/roottreemanifestid 2> /dev/null
  malformed url (no-eol)
  $ curl http://localhost:3000/repo/cs/3903775176ed42b1458a6281db4a0ccf4d9f287a/roottreemanifestid/more 2> /dev/null
  malformed url (no-eol)
  $ curl http://localhost:3000/repo/cs/ 2> /dev/null
  malformed url (no-eol)
  $ curl http://localhost:3000/ 2> /dev/null
  malformed url (no-eol)

Make sure there are no errors on the server
  $ cat $TESTTMP/edenserver.out
