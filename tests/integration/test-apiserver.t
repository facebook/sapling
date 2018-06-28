  $ CACHEDIR=$PWD/cachepath
  $ . $TESTDIR/library.sh

setup config repo
  $ setup_common_config
  $ cd $TESTTMP

setup testing repo for mononoke
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ TEST_CONTENT=$(cat /dev/urandom | tr -dc 'a-zA-Z0-9' | fold -w 1000 | head -n 1)
  $ echo $TEST_CONTENT >> test
  $ ln -s test link
  $ mkdir folder
  $ touch folder/.keep
  $ hg add test link folder/.keep
  $ hg commit -ma
  $ COMMIT1=$(hg --debug id -i)
  $ hg mv test test-rename
  $ hg commit -ma
  $ COMMIT2=$(hg --debug id -i)

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

starts api server
  $ apiserver -p 0

  $ for i in $(seq 1 40); do
  > PORT=$(cat $TESTTMP/apiserver.out | grep "Listening to" | grep -Pzo "(\\d+)\$") && break
  > sleep 0.1
  > done

test cat file
  $ curl http://127.0.0.1:$PORT/repo/blob/$COMMIT1/test 2> /dev/null > output
  $ diff output - <<< $TEST_CONTENT

test link file (no follow)
  $ curl http://127.0.0.1:$PORT/repo/blob/$COMMIT1/link 2> /dev/null
  test (no-eol)

test folder
  $ curl -w "\n%{http_code}" http://127.0.0.1:$PORT/repo/blob/$COMMIT1/folder 2> /dev/null
  folder is invalid
  400 (no-eol)

test cat renamed file
  $ curl http://127.0.0.1:$PORT/repo/blob/$COMMIT2/test-rename 2> /dev/null > output
  $ diff output - <<< $TEST_CONTENT

  $ curl -w "\n%{http_code}" http://127.0.0.1:$PORT/repo/blob/$COMMIT2/test 2> /dev/null
  test not found
  404 (no-eol)

  $ curl http://127.0.0.1:$PORT/status 2> /dev/null
  ok (no-eol)

  $ curl -w "\n%{http_code}" http://127.0.0.1:$PORT/repo/blob/0000000000000000000000000000000000000001/test 2> /dev/null
  0000000000000000000000000000000000000001 not found
  404 (no-eol)

  $ curl -w "\n%{http_code}" http://127.0.0.1:$PORT/other/blob/0000000000000000000000000000000000000001/test 2> /dev/null
  other not found
  404 (no-eol)

  $ curl -w "\n%{http_code}" http://127.0.0.1:$PORT/repo/blob/0000/test 2> /dev/null
  0000 is invalid
  400 (no-eol)

  $ curl -i http://127.0.0.1:$PORT//blob/000/test 2> /dev/null | grep 404
  HTTP/1.1 404 Not Found\r (esc)

  $ curl -i http://127.0.0.1:$PORT/sup/blob/ 2> /dev/null | grep 404
  HTTP/1.1 404 Not Found\r (esc)
