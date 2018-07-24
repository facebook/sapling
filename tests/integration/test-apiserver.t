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
  $ touch branch1
  $ hg add branch1
  $ hg commit -ma
  $ COMMITB1=$(hg --debug id -i)
  $ hg co $COMMIT2 > /dev/null
  $ touch branch2
  $ hg add branch2
  $ hg commit -ma
  $ COMMITB2=$(hg --debug id -i)
  $ COMMITB2_BOOKMARK=B2
  $ hg bookmark $COMMITB2_BOOKMARK

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

starts api server
  $ apiserver -p 0

  $ for i in $(seq 1 40); do
  > PORT=$(cat $TESTTMP/apiserver.out | grep "Listening to" | grep -Pzo "(\\d+)\$") && break
  > sleep 0.1
  > done

  $ if [[ -z "$PORT" ]]; then
  >   echo "error: Mononoke API Server is not started"
  >   cat $TESTTMP/apiserver.out
  >   exit 1
  > fi

  $ APISERVER="https://localhost:$PORT"

ping test
  $ sslcurl -i $APISERVER/status 2> /dev/null | grep -iv "date"
  HTTP/2 200 \r (esc)
  content-length: 2\r (esc)
  \r (esc)
  ok

test cat file
  $ sslcurl $APISERVER/repo/raw/$COMMIT1/test 2> /dev/null > output
  $ diff output - <<< $TEST_CONTENT

test link file (no follow)
  $ sslcurl $APISERVER/repo/raw/$COMMIT1/link 2> /dev/null
  test (no-eol)

test folder
  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/$COMMIT1/folder 2> /dev/null
  folder is invalid
  400 (no-eol)

test cat renamed file
  $ sslcurl $APISERVER/repo/raw/$COMMIT2/test-rename 2> /dev/null > output
  $ diff output - <<< $TEST_CONTENT

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/$COMMIT2/test 2> /dev/null
  test not found
  404 (no-eol)

  $ sslcurl $APISERVER/status 2> /dev/null
  ok (no-eol)

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/0000000000000000000000000000000000000001/test 2> /dev/null
  0000000000000000000000000000000000000001 not found
  404 (no-eol)

  $ sslcurl -w "\n%{http_code}" $APISERVER/other/raw/0000000000000000000000000000000000000001/test 2> /dev/null
  other not found
  404 (no-eol)

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/0000/test 2> /dev/null
  0000 is invalid
  400 (no-eol)

  $ sslcurl -i $APISERVER//raw/000/test 2> /dev/null | grep 404
  HTTP/2 404 \r (esc)

  $ sslcurl -i $APISERVER/sup/raw/ 2> /dev/null | grep 404
  HTTP/2 404 \r (esc)

test reachability in basic repo
  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT1/$COMMIT2 2> /dev/null
  true (no-eol)

  $ sslcurl  $APISERVER/repo/is_ancestor/$COMMIT2/$COMMIT1 2> /dev/null
  false (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT1/$COMMITB1 2> /dev/null
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT1/$COMMITB2 2> /dev/null
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT2/$COMMITB1 2> /dev/null
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT2/$COMMITB2 2> /dev/null
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMITB2/$COMMITB1 2> /dev/null
  false (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMITB1/$COMMITB2 2> /dev/null
  false (no-eol)

test reachability response on nonexistent nodes
  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/$COMMIT1/0000 2> /dev/null
  0000 is invalid
  400 (no-eol)

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/1111/$COMMIT2 2> /dev/null
  1111 is invalid
  400 (no-eol)

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/1111/2222 2> /dev/null
  2222 is invalid
  400 (no-eol)

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/0123456789123456789012345678901234567890/$COMMIT1 2> /dev/null
  0123456789123456789012345678901234567890 not found
  404 (no-eol)

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/$COMMIT2/1234567890123456789012345678901234567890 2> /dev/null
  1234567890123456789012345678901234567890 not found
  404 (no-eol)

test reachability on bookmarks
  $ echo $COMMITB2_BOOKMARK
  B2

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT2/$COMMITB2_BOOKMARK 2> /dev/null
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMITB2_BOOKMARK/$COMMITB2_BOOKMARK 2> /dev/null
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMITB2_BOOKMARK/$COMMIT2 2> /dev/null
  false (no-eol)
