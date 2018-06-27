
  $ CACHEDIR=$PWD/cachepath
  $ . $TESTDIR/library.sh

  $ cat >> $TESTTMP/json_pretty_print.py <<EOF
  > import sys
  > import json
  > inp = json.loads(sys.stdin.read())
  > inp = sorted(inp, key=lambda entry: entry['path'])
  > print(json.dumps(inp, sort_keys=True, indent=2, separators=(',', ': ')))
  > EOF

  $ json_print() {
  >   python $TESTTMP/json_pretty_print.py
  > }

From https://unix.stackexchange.com/questions/55913/whats-the-easiest-way-to-find-an-unused-local-port
  $ cat >> $TESTTMP/get_free_socket.py <<EOF
  > import socket
  > s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
  > s.bind(('', 0))
  > addr = s.getsockname()
  > print(addr[1])
  > s.close()
  > EOF

  $ hg init repo
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > [experimental]
  > evolution=createmarkers
  > EOF

  $ touch a
  $ hg add a
  $ hg ci -ma

  $ echo '1' > b
  $ echo 2 > c
  $ hg add b c
  $ hg ci -mb

Add commit with a copy
  $ hg cp c d
  $ hg ci -mc

Add commit with null manifest
  $ hg up null
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  $ echo 1 > 1
  $ hg add 1
  $ hg ci -m 'null manifest'
  $ hg rm 1
  $ hg commit --amend --traceback
  $ hg log -r 7f48e9c786d1 -T '{node}'
  7f48e9c786d1cbab525424e45139585724f84e28 (no-eol)
  $ hg debugdata -c 7f48e9c786d1cbab525424e45139585724f84e28
  0000000000000000000000000000000000000000
  test
  0 0 amend_source:813c7514ad5e14493de885987c241c14c5cd3153
  
  null manifest (no-eol)

Add commit with a directory
  $ mkdir dir
  $ echo content > dir/content
  $ hg add dir/content
  $ hg ci -m 'commit with dir'

  $ hg log
  changeset:   5:617e87e2aa2f
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit with dir
  
  changeset:   4:7f48e9c786d1
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     null manifest
  
  changeset:   2:533267b0e203
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  changeset:   1:4dabaf45f54a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ cd ..
  $ SOCKET=`python $TESTTMP/get_free_socket.py`
  $ mkdir $TESTTMP/blobrepo
  $ echo 'reponame="repo"' >> $TESTTMP/config
  $ echo "path=\"$TESTTMP/blobrepo\"" >> $TESTTMP/config
  $ echo "addr='127.0.0.1:$SOCKET'" >> $TESTTMP/config
  $ echo 'repotype="blob:rocks"' >> $TESTTMP/config
  $ echo 'repoid=0' >> $TESTTMP/config
  $ echo "[ssl]" >> $TESTTMP/config
  $ echo "cert=\"$TESTDIR/testcert.crt\"" >> $TESTTMP/config
  $ echo "private_key=\"$TESTDIR/testcert.key\"" >> $TESTTMP/config
  $ echo "ca_pem_file=\"$TESTDIR/testcert.crt\"" >> $TESTTMP/config
 
  $ blobimport $TESTTMP/repo/.hg $TESTTMP/blobrepo --debug
  $ grep -Eo 'inserted: .*' < $TESTTMP/blobimport.out | sort
  inserted: 3903775176ed42b1458a6281db4a0ccf4d9f287a
  inserted: 4dabaf45f54add88ca2797dfdeb00a7d55144243
  inserted: 533267b0e203537fa53d2aec834b062f0b2249cd
  inserted: 617e87e2aa2fe36508e8d5e15a162bcd2e79808e
  inserted: 7f48e9c786d1cbab525424e45139585724f84e28
  inserted: 813c7514ad5e14493de885987c241c14c5cd3153

  $ edenserver --config-file $TESTTMP/config

Curl and debugdata output should match
  $ alias curl="curl --cert $TESTDIR/testcert.crt --key $TESTDIR/testcert.key --cacert $TESTDIR/testcert.crt"

Wait at most 4 secs until server is ready
  $ for i in `seq 1 40`; do
  > curl https://localhost:$SOCKET > /dev/null 2>&1 && break
  > sleep 0.1
  > done

Send requests to the server
  $ curl https://localhost:$SOCKET/repo/cs/3903775176ed42b1458a6281db4a0ccf4d9f287a/roottreemanifestid 2> /dev/null
  8515d4bfda768e04af4c13a69a72e28c7effbea7 (no-eol)
  $ cd repo
  $ hg debugdata -c 3903775176ed42b1458a6281db4a0ccf4d9f287a | head -n 1
  8515d4bfda768e04af4c13a69a72e28c7effbea7
  $ hg debugdata -m 8515d4bfda768e04af4c13a69a72e28c7effbea7
  a\x00b80de5d138758541c5f05265ad144ab9fa86d1db (esc)
  $ curl https://localhost:$SOCKET/repo/cs/533267b0e203537fa53d2aec834b062f0b2249cd/roottreemanifestid 2> /dev/null
  47827ecc7f12d2ed0c387de75947e73cf1c53afe (no-eol)

  $ hg debugdata -m 47827ecc7f12d2ed0c387de75947e73cf1c53afe
  a\x00b80de5d138758541c5f05265ad144ab9fa86d1db (esc)
  b\x00b8e02f6433738021a065f94175c7cd23db5f05be (esc)
  c\x005d9299349fc01ddd25d0070d149b124d8f10411e (esc)
  d\x00fc702583f9c961dea176fd367862c299b4a551f2 (esc)

  $ curl https://localhost:$SOCKET/repo/treenode/8515d4bfda768e04af4c13a69a72e28c7effbea7/ 2> /dev/null| json_print
  [
    {
      "hash": "b80de5d138758541c5f05265ad144ab9fa86d1db",
      "path": "a",
      "size": 0,
      "type": "File"
    }
  ]

Empty file
  $ curl https://localhost:$SOCKET/repo/blob/b80de5d138758541c5f05265ad144ab9fa86d1db/ 2> /dev/null

  $ curl https://localhost:$SOCKET/repo/cs/4dabaf45f54add88ca2797dfdeb00a7d55144243/roottreemanifestid 2> /dev/null
  b47dc781a873595c796b01e2ed5829e3fed2c887 (no-eol)
  $ curl https://localhost:$SOCKET/repo/treenode/b47dc781a873595c796b01e2ed5829e3fed2c887/ 2> /dev/null| json_print
  [
    {
      "hash": "b80de5d138758541c5f05265ad144ab9fa86d1db",
      "path": "a",
      "size": 0,
      "type": "File"
    },
    {
      "hash": "b8e02f6433738021a065f94175c7cd23db5f05be",
      "path": "b",
      "size": 2,
      "type": "File"
    },
    {
      "hash": "5d9299349fc01ddd25d0070d149b124d8f10411e",
      "path": "c",
      "size": 2,
      "type": "File"
    }
  ]
  $ curl https://localhost:$SOCKET/repo/blob/5d9299349fc01ddd25d0070d149b124d8f10411e/ 2> /dev/null
  2
  $ curl https://localhost:$SOCKET/repo/treenode/47827ecc7f12d2ed0c387de75947e73cf1c53afe/ 2> /dev/null | json_print
  [
    {
      "hash": "b80de5d138758541c5f05265ad144ab9fa86d1db",
      "path": "a",
      "size": 0,
      "type": "File"
    },
    {
      "hash": "b8e02f6433738021a065f94175c7cd23db5f05be",
      "path": "b",
      "size": 2,
      "type": "File"
    },
    {
      "hash": "5d9299349fc01ddd25d0070d149b124d8f10411e",
      "path": "c",
      "size": 2,
      "type": "File"
    },
    {
      "hash": "fc702583f9c961dea176fd367862c299b4a551f2",
      "path": "d",
      "size": 2,
      "type": "File"
    }
  ]
  $ curl https://localhost:$SOCKET/repo/blob/fc702583f9c961dea176fd367862c299b4a551f2/ 2> /dev/null
  2

  $ curl https://localhost:$SOCKET/repo/cs/617e87e2aa2fe36508e8d5e15a162bcd2e79808e/roottreemanifestid 2> /dev/null
  ed8f515856d818e78bd52edac84a97568de65e0f (no-eol)

  $ curl https://localhost:$SOCKET/repo/cs/617e87e2aa2fe36508e8d5e15a162bcd2e79808e/roottreemanifestid/ 2> /dev/null
  ed8f515856d818e78bd52edac84a97568de65e0f (no-eol)

  $ curl https://localhost:$SOCKET/repo/treenode/ed8f515856d818e78bd52edac84a97568de65e0f/ 2> /dev/null | json_print
  [
    {
      "hash": "e7405b0462d8b2dd80219b713a93aea2c9a3c468",
      "path": "dir",
      "size": null,
      "type": "Tree"
    }
  ]

  $ curl https://localhost:$SOCKET/repo/treenode/e7405b0462d8b2dd80219b713a93aea2c9a3c468/ 2> /dev/null | json_print
  [
    {
      "hash": "7108421418404a937c684d2479a34a24d2ce4757",
      "path": "content",
      "size": 8,
      "type": "File"
    }
  ]
  $ curl https://localhost:$SOCKET/repo/treenode/e7405b0462d8b2dd80219b713a93aea2c9a3c468 2> /dev/null | json_print
  [
    {
      "hash": "7108421418404a937c684d2479a34a24d2ce4757",
      "path": "content",
      "size": 8,
      "type": "File"
    }
  ]

treenode_simple - do not request sizes
  $ curl https://localhost:$SOCKET/repo/treenode_simple/e7405b0462d8b2dd80219b713a93aea2c9a3c468 2> /dev/null | json_print
  [
    {
      "hash": "7108421418404a937c684d2479a34a24d2ce4757",
      "path": "content",
      "size": null,
      "type": "File"
    }
  ]

  $ curl https://localhost:$SOCKET/repo/blob/7108421418404a937c684d2479a34a24d2ce4757/ 2> /dev/null
  content
  $ curl https://localhost:$SOCKET/repo/blob/7108421418404a937c684d2479a34a24d2ce4757 2> /dev/null
  content

Send incorrect requests
  $ curl https://localhost:$SOCKET/repo/cs/hash/roottreemanifestid 2> /dev/null
  invalid sha-1 input: need at least 40 hex digits (no-eol)
  $ curl https://localhost:$SOCKET/badrepo/cs/3903775176ed42b1458a6281db4a0ccf4d9f287a/roottreemanifestid 2> /dev/null
  Error: unknown repo
  $ curl https://localhost:$SOCKET/badrepo/treenode/8515d4bfda768e04af4c13a69a72e28c7effbea7/ 2> /dev/null
  Error: unknown repo
  $ curl https://localhost:$SOCKET/repo/BADURL/3903775176ed42b1458a6281db4a0ccf4d9f287a/roottreemanifestid 2> /dev/null
  malformed url (no-eol)
  $ curl https://localhost:$SOCKET/repo/cs/3903775176ed42b1458a6281db4a0ccf4d9f287a/roottreemanifestid/more 2> /dev/null
  malformed url (no-eol)
  $ curl https://localhost:$SOCKET/repo/cs/ 2> /dev/null
  malformed url (no-eol)
  $ curl https://localhost:$SOCKET/ 2> /dev/null
  malformed url (no-eol)

Make sure there are no errors on the server
  $ cat $TESTTMP/edenserver.out
  I*scm/mononoke/eden_server/src/main.rs:*] started eden server (glob)
