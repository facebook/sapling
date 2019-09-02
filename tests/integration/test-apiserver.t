  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

setup config repo
  $ setup_common_config
  $ cd $TESTTMP

setup testing repo for mononoke
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  >>> import os, textwrap, base64
  >>> open('test', 'w').write(textwrap.fill(base64.b64encode(os.urandom(10000))) + "\n")
  $ TEST_CONTENT=$(cat test)
  $ SHA=$(sha256sum test | awk '{print $1;}')
  $ ln -s test link
  $ mkdir -p folder/subfolder
  $ echo "hello" > folder/subfolder/.keep
  $ echo "hello" > duplicate
  $ echo "hello" > duplicate-2
  $ hg add test link folder/subfolder/.keep duplicate duplicate-2
  $ hg commit -ma
  $ COMMIT1=$(hg --debug id -i)
  $ BLOBHASH=$(hg manifest --debug | grep test | cut -d' ' -f1)
  $ hg mv test test-rename
  $ hg commit -ma
  $ COMMIT2=$(hg --debug id -i)
  $ BLOBHASH_RENAMED=$(hg manifest --debug | grep test-rename | cut -d' ' -f1)
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
  $ blobimport repo-hg/.hg repo

starts api server
  $ APISERVER_PORT=$(get_free_socket)
  $ apiserver -H "[::1]" -p $APISERVER_PORT
  $ wait_for_apiserver
  $ function sslcurl() { curl --silent --cert "${TEST_CERTDIR}/localhost.crt" --cacert "${TEST_CERTDIR}/root-ca.crt" --key "${TEST_CERTDIR}/localhost.key" "$@"; }
  $ function s_client() { openssl s_client -connect $APIHOST -CAfile "${TEST_CERTDIR}/root-ca.crt" -cert "${TEST_CERTDIR}/localhost.crt" -key "${TEST_CERTDIR}/localhost.key" -ign_eof "$@"; }

ping test
  $ sslcurl -i $APISERVER/health_check | grep -iv "date"
  HTTP/* 200 * (glob)
  content-length: 10\r (esc)
  \r (esc)
  I_AM_ALIVE

hostname test
  $ sslcurl $APISERVER/hostname > output
  $ echo >> output # Add trailing newline.
  $ diff output - <<< $HOSTNAME

test cat file
  $ sslcurl $APISERVER/repo/raw/$COMMIT1/test > output
  $ diff output - <<< $TEST_CONTENT

test link file (no follow)
  $ sslcurl $APISERVER/repo/raw/$COMMIT1/link
  test (no-eol)

test folder
  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/$COMMIT1/folder | extract_json_error
  Invalid input: folder
  400

test cat renamed file
  $ sslcurl $APISERVER/repo/raw/$COMMIT2/test-rename > output
  $ diff output - <<< $TEST_CONTENT

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/$COMMIT2/test | extract_json_error
  test is not found
  404

  $ sslcurl $APISERVER/health_check
  I_AM_ALIVE (no-eol)

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/0000000000000000000000000000000000000001/test | extract_json_error
  0000000000000000000000000000000000000001 is not found
  404

  $ sslcurl -w "\n%{http_code}" $APISERVER/other/raw/0000000000000000000000000000000000000001/test | extract_json_error
  other is not found
  404

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/raw/0000/test | extract_json_error
  Invalid input: 0000
  400

  $ sslcurl -i $APISERVER//raw/000/test 2> /dev/null | grep 404
  HTTP/* 404 * (glob)

  $ sslcurl -i $APISERVER/sup/raw/ 2> /dev/null | grep 404
  HTTP/* 404 * (glob)

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
  Invalid input: 0000
  400

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/1111/$COMMIT2 | extract_json_error
  Invalid input: 1111
  400

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/1111/2222 | extract_json_error
  Invalid input: 2222
  400

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/0123456789123456789012345678901234567890/$COMMIT1 | extract_json_error
  0123456789123456789012345678901234567890 is not found
  404

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/is_ancestor/$COMMIT2/1234567890123456789012345678901234567890 | extract_json_error
  1234567890123456789012345678901234567890 is not found
  404

test folder list
  $ sslcurl $APISERVER/repo/list/$COMMIT2/folder | tee output | jq .
  [
    {
      "name": "subfolder",
      "type": "tree",
      "hash": "732eacf2be3265bd6bc4d2c205434b280f446cbf"
    }
  ]

  $ TREEHASH=$(cat output | jq -r ".[0].hash")

  $ sslcurl $APISERVER/repo/list/$COMMIT2/folder/subfolder | jq .
  [
    {
      "name": ".keep",
      "type": "file",
      "hash": "2c186c8c5bc0df5af5b951afe407d803f9e6b8c9"
    }
  ]

test nonexist fold
  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/list/$COMMIT2/nonexist | extract_json_error
  nonexist is not found
  404

test list a file
  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/list/$COMMIT2/test-rename | extract_json_error
  test-rename is not a directory
  400

test get blob by hash
  $ sslcurl $APISERVER/repo/blob/$BLOBHASH > output
  $ diff output - <<< $TEST_CONTENT

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/blob/$TREEHASH | extract_json_error
  732eacf2be3265bd6bc4d2c205434b280f446cbf is not found
  404

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/blob/0000 | extract_json_error
  Invalid input: 0000
  400

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/blob/0000000000000000000000000000000000000001 | extract_json_error
  0000000000000000000000000000000000000001 is not found
  404

test get tree
  $ sslcurl $APISERVER/repo/tree/$TREEHASH | jq .
  [
    {
      "name": ".keep",
      "type": "file",
      "hash": "2c186c8c5bc0df5af5b951afe407d803f9e6b8c9",
      "size": 6,
      "content_sha1": "f572d396fae9206628714fb2ce00f72e94f2258f"
    }
  ]

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/tree/$BLOBHASH | extract_json_error > output
  $ echo -e "$BLOBHASH is not found\n404" > baseline
  $ diff output baseline

  $ sslcurl -w "\n%{http_code}" $APISERVER/repo/tree/0000 | extract_json_error
  Invalid input: 0000
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
    "name": "duplicate",
    "type": "file"
  }
  {
    "name": "duplicate-2",
    "type": "file"
  }
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
  Invalid input: 0000
  400

test TLS Session/Ticket resumption when using client certs
  $ TMPFILE=$(mktemp)
  $ RUN1=$(echo -e "GET /health_check HTTP/1.1\r\n" | s_client -sess_out $TMPFILE | grep -E "^(HTTP|\s+Session-ID:)")
  depth=1 C = US, ST = CA, O = FakeRootCanal, CN = fbmononoke.com
  verify return:1
  depth=0 CN = localhost, O = Mononoke, C = US, ST = CA
  verify return:1
  $ RUN2=$(echo -e "GET /health_check HTTP/1.1\r\n" | s_client -sess_in $TMPFILE | grep -E "^(HTTP|\s+Session-ID:)")
  $ echo "$RUN1"
      Session-ID: [A-Z0-9]{64} (re)
  HTTP/1.1 200 OK\r (esc)
  $ if [ "$RUN1" == "$RUN2" ]; then echo "SUCCESS"; fi
  SUCCESS

test TLS Tickets use encryption keys from seeds - sessions should persist across restarts
  $ kill -9 $APISERVER_PID && wait $APISERVER_PID
  $TESTTMP.sh: * Killed * (glob)
  [137]
  $ truncate -s 0 "$TESTTMP/apiserver.out"
  $ apiserver -H "[::1]" -p $APISERVER_PORT
  $ wait_for_apiserver
  $ echo -e "GET /health_check HTTP/1.1\r\n" | s_client -sess_in $TMPFILE -state | grep -E "^SSL_connect"
  SSL_connect:before/connect initialization
  SSL_connect:SSLv3 write client hello A
  SSL_connect:SSLv3 read server hello A
  SSL_connect:SSLv3 read finished A
  SSL_connect:SSLv3 write change cipher spec A
  SSL_connect:SSLv3 write finished A
  SSL_connect:SSLv3 flush data
  SSL3 alert read:warning:close notify
  SSL3 alert write:warning:close notify
  [1]

test download LFS (GET request)
  $ sslcurl $APISERVER/repo/lfs/download/$SHA > output
  $ diff output - <<< $TEST_CONTENT

  $ NON_EXISTING_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ sslcurl  -w "\n%{http_code}" $APISERVER/repo/lfs/download/$NON_EXISTING_SHA | extract_json_error
  internal server error: Content blob missing for id: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  500

  $ NON_VALID_SHA="1234"
  $ sslcurl  -w "\n%{http_code}" $APISERVER/repo/lfs/download/$NON_VALID_SHA | extract_json_error
  Invalid input: 1234
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
  $ sslcurl -d '{"operation": "download","transfers":["basic"],"objects":[{"oid": "12345678","size": 23}]}'  --http1.1 -H "Content-Type: application/json" -X POST $APISERVER/repo/objects/batch > output
  $ sed -i -e '$a\' output
  $ diff -c output - <<< $EXPECTED_OUTPUT

batch for unknown repo
  $ sslcurl -d '{"operation": "download","transfers":["basic"],"objects":[{"oid": "12345678","size": 23}]}' -H "Content-Type: application/json" -X POST $APISERVER/unknown_repo/objects/batch | jq '.message'
  "unknown_repo is not found on LFS request"
