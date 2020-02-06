# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

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

test TLS Session/Ticket resumption when using client certs
  $ TMPFILE=$(mktemp)
  $ RUN1=$(echo -e "hello\n" | s_client -sess_out $TMPFILE | grep -E "^(HTTP|\s+Session-ID:)")
  Can't use SSL_get_servername
  depth=1 C = US, ST = CA, O = FakeRootCanal, CN = fbmononoke.com
  verify return:1
  depth=0 CN = localhost, O = Mononoke, C = US, ST = CA
  verify return:1
  read:errno=0
  $ RUN2=$(echo -e "hello\n" | s_client -sess_in $TMPFILE | grep -E "^(HTTP|\s+Session-ID:)")
  Can't use SSL_get_servername
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
  $ echo -e "hello\n" | s_client -sess_in $TMPFILE -state | grep -E "^SSL_connect"
  SSL_connect:before SSL initialization
  SSL_connect:SSLv3/TLS write client hello
  SSL_connect:SSLv3/TLS write client hello
  Can't use SSL_get_servername
  SSL_connect:SSLv3/TLS read server hello
  SSL_connect:SSLv3/TLS read change cipher spec
  SSL_connect:SSLv3/TLS read finished
  SSL_connect:SSLv3/TLS write change cipher spec
  SSL_connect:SSLv3/TLS write finished
  read:errno=0
  SSL3 alert write:warning:close notify
  [1]
