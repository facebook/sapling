  $ CACHEDIR=$PWD/cachepath
  $ . $TESTDIR/library.sh

setup config repo
  $ setup_common_config
  $ cd $TESTTMP

setup testing repo for mononoke
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

starts api server
  $ apiserver -p 0

  $ for i in $(seq 1 40); do
  > PORT=$(cat $TESTTMP/apiserver.out | grep "Listening to" | grep -Pzo "(\\d+)\$") && break
  > sleep 0.1
  > done

  $ curl http://127.0.0.1:$PORT/repo/blob/test 2> /dev/null
  success! (no-eol)

  $ curl -i http://127.0.0.1:$PORT//blob/test 2> /dev/null | grep 404
  HTTP/1.1 404 Not Found\r (esc)

  $ curl -i http://127.0.0.1:$PORT/sup/blob/ 2> /dev/null | grep 404
  HTTP/1.1 404 Not Found\r (esc)
