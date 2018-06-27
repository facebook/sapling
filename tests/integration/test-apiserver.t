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
  $ hg add test
  $ hg commit -ma
  $ HGHASH1=$(hg manifest --debug | cut -d' ' -f 1  | head -1)
  $ hg mv test test-rename
  $ hg commit -ma
  $ HGHASH2=$(hg manifest --debug | cut -d' ' -f 1  | head -1)

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
  $ curl http://127.0.0.1:$PORT/repo/blob/$HGHASH1 2> /dev/null > output
  $ diff output - <<< $TEST_CONTENT

test cat renamed file
  $ curl http://127.0.0.1:$PORT/repo/blob/$HGHASH2 2> /dev/null > output
  $ diff output - <<< $TEST_CONTENT

  $ curl -w "\n%{http_code}" http://127.0.0.1:$PORT/repo/blob/0000000000000000000000000000000000000000 2> /dev/null
  0000000000000000000000000000000000000000 not found
  404 (no-eol)

  $ curl -w "\n%{http_code}" http://127.0.0.1:$PORT/other/blob/0000000000000000000000000000000000000000 2> /dev/null
  other not found
  404 (no-eol)

  $ curl -w "\n%{http_code}" http://127.0.0.1:$PORT/repo/blob/0000 2> /dev/null
  0000 is invalid
  400 (no-eol)

  $ curl -i http://127.0.0.1:$PORT//blob/test 2> /dev/null | grep 404
  HTTP/1.1 404 Not Found\r (esc)

  $ curl -i http://127.0.0.1:$PORT/sup/blob/ 2> /dev/null | grep 404
  HTTP/1.1 404 Not Found\r (esc)
