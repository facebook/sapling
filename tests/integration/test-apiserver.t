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
  $ SHA=$(sha256sum test | awk '{print $1;}')
  $ ln -s test link
  $ mkdir -p folder/subfolder
  $ touch folder/subfolder/.keep
  $ hg add test link folder/subfolder/.keep
  $ hg commit -ma
  $ COMMIT1=$(hg --debug id -i)
  $ BLOBHASH=$(hg manifest --debug | grep test | cut -d' ' -f1)
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
  $ touch forward_slash_bm
  $ hg add forward_slash_bm
  $ hg commit -ma
  $ FORWARD_SLASH_BM_HASH=$(hg --debug id -i)
  $ COMMITB2_BOOKMARK=B2
  $ hg co $COMMITB2 > /dev/null
  $ hg bookmark $COMMITB2_BOOKMARK
  $ hg co $FORWARD_SLASH_BM_HASH > /dev/null
  $ FORWARD_SLASH_BM=forward/slash/bookmark
  $ ENCODED_FORWARD_SLASH_BM=forward%2Fslash%2Fbookmark
  $ hg bookmark $FORWARD_SLASH_BM

import testing repo to mononoke
  $ cd ..
  $ blobimport rocksdb repo-hg/.hg repo

starts api server
  $ apiserver -H "127.0.0.1" -p $(get_free_socket)
  $ wait_for_apiserver
  $ alias sslcurl="sslcurl --silent"

ping test
  $ sslcurl -i $APISERVER/status | grep -iv "date"
  HTTP/1.1 200 OK\r (esc)
  content-length: 2\r (esc)
  \r (esc)
  ok

test cat file
  $ sslcurl $APISERVER/repo/raw/$COMMIT1/test > output
  $ diff output - <<< $TEST_CONTENT

test link file (no follow)
  $ sslcurl $APISERVER/repo/raw/$COMMIT1/link
  test (no-eol)

test folder
  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/$COMMIT1/folder | extract_json_error
  folder is invalid
  400

test cat renamed file
  $ sslcurl $APISERVER/repo/raw/$COMMIT2/test-rename > output
  $ diff output - <<< $TEST_CONTENT

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/$COMMIT2/test | extract_json_error
  test is not found
  404

  $ sslcurl $APISERVER/status
  ok (no-eol)

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/0000000000000000000000000000000000000001/test | extract_json_error
  0000000000000000000000000000000000000001 is not found
  404

  $ sslcurl -w "\n%{http_code}" $APISERVER/other/raw/0000000000000000000000000000000000000001/test | extract_json_error
  other is not found
  404

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/0000/test | extract_json_error
  0000 is invalid
  400

  $ sslcurl -i $APISERVER//raw/000/test 2> /dev/null | grep 404
  HTTP/1.1 404 Not Found\r (esc)

  $ sslcurl -i $APISERVER/sup/raw/ 2> /dev/null | grep 404
  HTTP/1.1 404 Not Found\r (esc)

test reachability in basic repo
  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT1/$COMMIT2
  true (no-eol)

  $ sslcurl  $APISERVER/repo/is_ancestor/$COMMIT2/$COMMIT1
  false (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT1/$COMMITB1
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT1/$COMMITB2
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT2/$COMMITB1
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT2/$COMMITB2
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMITB2/$COMMITB1
  false (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMITB1/$COMMITB2
  false (no-eol)

test reachability response on nonexistent nodes
  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/$COMMIT1/0000 | extract_json_error
  0000 is invalid
  400

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/1111/$COMMIT2 | extract_json_error
  1111 is invalid
  400

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/1111/2222 | extract_json_error
  2222 is invalid
  400

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/0123456789123456789012345678901234567890/$COMMIT1 | extract_json_error
  0123456789123456789012345678901234567890 is not found
  404

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/$COMMIT2/1234567890123456789012345678901234567890 | extract_json_error
  1234567890123456789012345678901234567890 is not found
  404

test reachability on bookmarks
  $ echo $COMMITB2_BOOKMARK
  B2

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT2/$COMMITB2_BOOKMARK
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMITB2_BOOKMARK/$COMMITB2_BOOKMARK
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMITB2_BOOKMARK/$COMMIT2
  false (no-eol)

test reachability on url encoded bookmarks

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMIT2/$ENCODED_FORWARD_SLASH_BM
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$ENCODED_FORWARD_SLASH_BM/$COMMIT2
  false (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$COMMITB2_BOOKMARK/$ENCODED_FORWARD_SLASH_BM
  true (no-eol)

  $ sslcurl $APISERVER/repo/is_ancestor/$ENCODED_FORWARD_SLASH_BM/$COMMITB2_BOOKMARK
  false (no-eol)

test folder list
  $ sslcurl $APISERVER/repo/list/$COMMIT2/folder | tee output | python -mjson.tool
  [
      {
          "name": "subfolder",
          "type": "tree",
          "hash": "9b5497965e634f261cca0247a7a48b709a7be2b9"
      }
  ]

  $ TREEHASH=$(cat output | jq -r ".[0].hash")

  $ sslcurl $APISERVER/repo/list/$COMMIT2/folder/subfolder | python -mjson.tool
  [
      {
          "name": ".keep",
          "type": "file",
          "hash": "b80de5d138758541c5f05265ad144ab9fa86d1db"
      }
  ]

test nonexist fold
  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/list/$COMMIT2/nonexist | extract_json_error
  nonexist is not found
  404

test list a file
  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/list/$COMMIT2/test-rename | extract_json_error
  test-rename is invalid
  400

test get blob by hash
  $ sslcurl $APISERVER/repo/blob/$BLOBHASH > output
  $ diff output - <<< $TEST_CONTENT

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/blob/$TREEHASH | extract_json_error
  9b5497965e634f261cca0247a7a48b709a7be2b9 is not found
  404

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/blob/0000 | extract_json_error
  0000 is invalid
  400

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/blob/0000000000000000000000000000000000000001 | extract_json_error
  0000000000000000000000000000000000000001 is not found
  404

test get tree
  $ sslcurl $APISERVER/repo/tree/$TREEHASH | python -mjson.tool
  [
      {
          "name": ".keep",
          "type": "file",
          "hash": "b80de5d138758541c5f05265ad144ab9fa86d1db"
      }
  ]

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/tree/$BLOBHASH | extract_json_error > output
  $ echo -e "$BLOBHASH is not found\n404" > baseline
  $ diff output baseline

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/tree/0000 | extract_json_error
  0000 is invalid
  400

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/tree/0000000000000000000000000000000000000001 | extract_json_error
  0000000000000000000000000000000000000001 is not found
  404

test get changeset
  $ sslcurl $APISERVER/repo/changeset/$COMMIT1 | tee output | jq ".comment,.author"
  "a"
  "test"

  $ sslcurl $APISERVER/repo/tree/$(cat output | jq -r ".manifest") | jq ".[] | {name, type}"
  {
    "name": "folder",
    "type": "tree"
  }
  {
    "name": "link",
    "type": "symlink"
  }
  {
    "name": "test",
    "type": "file"
  }

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/changeset/0000000000000000000000000000000000000001 | extract_json_error
  0000000000000000000000000000000000000001 is not found
  404

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/changeset/0000 | extract_json_error
  0000 is invalid
  400

test download LFS (GET request)
  $ sslcurl $APISERVER/repo/lfs/download/$SHA > output
  $ diff output - <<< $TEST_CONTENT

  $ NON_EXISTING_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ sslcurl  -w "\n%{http_code}" $APISERVER/repo/lfs/download/$NON_EXISTING_SHA | extract_json_error
  internal server error: Missing typed key entry for key: alias.sha256.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  500

  $ NON_VALID_SHA="1234"
  $ sslcurl  -w "\n%{http_code}" $APISERVER/repo/lfs/download/$NON_VALID_SHA | extract_json_error
  1234 is invalid
  400

test upload+download LFS (PUT request)
  $ LFS_UPLOAD_FILE_CONTENT="lfs-upload-file-content"
  $ echo $LFS_UPLOAD_FILE_CONTENT > repo-hg/lfs-file
  $ LFS_SHA=$(sha256sum repo-hg/lfs-file | awk '{print $1;}')
  $ sslcurl -T repo-hg/lfs-file $APISERVER/repo/lfs/upload/$LFS_SHA

  $ sslcurl $APISERVER/repo/lfs/download/$LFS_SHA > output
  $ diff output - <<< $LFS_UPLOAD_FILE_CONTENT

test batch LFS
Replace localhost to 127.0.0.1, and add newline to curl output.
Be careful, if you want to cat the result of your curl operation, or whatever, ALL console prints are replaced with
127.0.0.1 -> $LOCALIP. Do not try to replace anything to $LOCALIP as a string.
USE od (octal dump) if you stuck with the issue.
  $ EXPECTED_OUTPUT="{\"transfer\":\"basic\",\"objects\":[{\"oid\":\"12345678\",\"size\":23,\"actions\":{\"download\":{\"href\":\"$APISERVER/repo/lfs/download/12345678\",\"expires_at\":\"2030-11-10T15:29:07Z\"}}}]}"
  $ sed s/localhost/127.0.0.1/ - <<< $EXPECTED_OUTPUT > expected_output_file
  $ sslcurl -d '{"operation": "download","transfers":["basic"],"objects":[{"oid": "12345678","size": 23}]}' -H "Content-Type: application/json" -X POST $APISERVER/repo/objects/batch > output
  $ sed -i -e '$a\' output
  $ diff -c output expected_output_file

batch for unknown repo
  $ sslcurl -d '{"operation": "download","transfers":["basic"],"objects":[{"oid": "12345678","size": 23}]}' -H "Content-Type: application/json" -X POST $APISERVER/unknown_repo/objects/batch | jq '.message'
  "unknown_repo is not found on LFS request"
