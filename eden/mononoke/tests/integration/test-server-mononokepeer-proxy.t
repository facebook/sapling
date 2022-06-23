# Copyright (c) Meta Platforms, Inc. and affiliates.
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

setup client repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-client
  $ cd repo-client
  $ setup_hg_client

start mononoke
  $ start_and_wait_for_mononoke_server
check if we can talk to mononoke through proxy
  $ PROXY_PORT=$(get_free_socket)
  $ ncat -lkv localhost $PROXY_PORT -c "tee -a ${TESTTMP}/ncat_proxy.log | ncat -v --ssl-cert $TEST_CERTDIR/proxy.crt --ssl-key $TEST_CERTDIR/proxy.key localhost ${MONONOKE_SOCKET} | tee -a ${TESTTMP}/ncat_proxy.log" 2>/dev/null &
  $ ncat_pid=$!
  $ cat >> .hg/hgrc <<EOF
  > [auth_proxy]
  > http_proxy=http://localhost:$PROXY_PORT
  > EOF

pull from mononoke and log data
  $ hgmn pull
  pulling from mononoke://$LOCALIP:*/repo (glob)
  searching for changes
  no changes found

check proxy
  $ cat ${TESTTMP}/ncat_proxy.log | grep -oa "HTTP/1.1 101 Switching Protocols"
  HTTP/1.1 101 Switching Protocols
  $ kill $ncat_pid
