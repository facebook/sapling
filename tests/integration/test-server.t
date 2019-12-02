setup
  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup data
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke
  $ function s_client () { openssl s_client -connect localhost:$MONONOKE_SOCKET -CAfile "${TEST_CERTDIR}/root-ca.crt" -cert "${TEST_CERTDIR}/localhost.crt" -key "${TEST_CERTDIR}/localhost.key" -ign_eof "$@"; }

test TLS Session/Ticket resumption when using client certs
  $ TMPFILE=$(mktemp)
  $ RUN1=$(echo -e "hello\n" | s_client -sess_out $TMPFILE | grep -E "^(HTTP|\s+Session-ID:)")
  depth=1 C = US, ST = CA, O = FakeRootCanal, CN = fbmononoke.com
  verify return:1
  depth=0 CN = localhost, O = Mononoke, C = US, ST = CA
  verify return:1
  read:errno=0
  $ RUN2=$(echo -e "hello\n" | s_client -sess_in $TMPFILE | grep -E "^(HTTP|\s+Session-ID:)")
  read:errno=0
  $ echo "$RUN1"
      Session-ID: [A-Z0-9]{64} (re)
  $ if [ "$RUN1" == "$RUN2" ]; then echo "SUCCESS"; fi
  SUCCESS

test TLS Tickets use encryption keys from seeds - sessions should persist across restarts
  $ kill -9 $MONONOKE_PID && wait $MONONOKE_PID
  $TESTTMP.sh: * Killed * (glob)
  [137]
  $ mononoke
  $ wait_for_mononoke
  $ alias s_client="openssl s_client -connect localhost:$MONONOKE_SOCKET -CAfile \"${TEST_CERTDIR}/root-ca.crt\" -cert \"${TEST_CERTDIR}/localhost.crt\" -key \"${TEST_CERTDIR}/localhost.key\" -ign_eof"
  $ echo -e "hello\n" | s_client -sess_in $TMPFILE -state | grep -E "^SSL_connect"
  SSL_connect:before/connect initialization
  SSL_connect:SSLv3 write client hello A
  SSL_connect:SSLv3 read server hello A
  SSL_connect:SSLv3 read finished A
  SSL_connect:SSLv3 write change cipher spec A
  SSL_connect:SSLv3 write finished A
  SSL_connect:SSLv3 flush data
  read:errno=0
  SSL3 alert write:warning:close notify
  [1]
