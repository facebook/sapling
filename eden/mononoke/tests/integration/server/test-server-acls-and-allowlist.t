# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ ADDITIONAL_MONONOKE_COMMON_CONFIG=$(cat <<EOF
  > [[global_allowlist]]
  > identity_type = "$CLIENT2_ID_TYPE"
  > identity_data = "$CLIENT2_ID_DATA"
  > EOF
  > )
  $ setup_common_config
  $ configure modern
  $ cd $TESTTMP

add client 2 to the global allowlist

setup repo
  $ testtool_drawdag -R repo --derive-all <<'EOF'
  > A
  > # bookmark: A main
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

start mononoke
  $ start_and_wait_for_mononoke_server

Clone the repo
  $ hgmn_clone  mononoke://$(mononoke_address)/repo repo
  $ cd repo

Pull with the default certificate - this should work.
  $ hgmn pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo

Pull from Mononoke with a different identity, make sure it fails
  $ hgmn pull --config auth.mononoke.cert="$TEST_CERTDIR/client1.crt" --config auth.mononoke.key="$TEST_CERTDIR/client1.key"
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  remote: Authorization failed: Unauthorized access, permission denied
  abort: unexpected EOL, expected netstring digit
  [255]

Pull with the identity in the global allowlist - this works, too.
  $ hgmn pull --config auth.mononoke.cert="$TEST_CERTDIR/client2.crt" --config auth.mononoke.key="$TEST_CERTDIR/client2.key"
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
