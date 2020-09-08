# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup a Mononoke repo.

  $ LFS_THRESHOLD="10" setup_common_config "blob_files"
  $ cd "$TESTTMP"

Start Mononoke & LFS. Require TLS certs in LFS.

  $ mononoke
  $ wait_for_mononoke
  $ lfs_url="$(lfs_server --tls --scuba-log-file "$TESTTMP/scuba.json" --allowed-test-identity "USER:myusername0")/repo"

Create a repo

  $ hgmn_init repo
  $ cd repo
  $ yes 2>/dev/null | head -c 100 > large
  $ hg add large
  $ hg ci -ma
  $ hgmn push -q --to master --create
  $ cd "$TESTTMP"

Clone the repo. Enable LFS. Take a different cache path to make sure we have to go to the server.

  $ hgmn_clone ssh://user@dummy/repo repo-clone --noupdate --config extensions.remotenames=
  $ cd repo-clone
  $ setup_hg_modern_lfs "$lfs_url" 10B
  $ setconfig "remotefilelog.cachepath=$TESTTMP/cachepath2"

Configure TLS

  $ setconfig "auth.lfs.cert=$TEST_CERTDIR/localhost.crt"
  $ setconfig "auth.lfs.key=$TEST_CERTDIR/localhost.key"
  $ setconfig "auth.lfs.cacerts=$TEST_CERTDIR/root-ca.crt"
  $ setconfig "auth.lfs.schemes=https"
  $ setconfig "auth.lfs.prefix=localhost"

Now, update. The server will send this file as LFS.

  $ hgmn up master -q
  $ test -f large

Finally, check what identities the client presented.

  $ wait_for_json_record_count "$TESTTMP/scuba.json" 2
  $ jq -S .normvector.client_identities "$TESTTMP/scuba.json"
  [
    "MACHINE:devvm000.lla0.facebook.com",
    "MACHINE_TIER:devvm",
    "USER:myusername0"
  ]
  [
    "MACHINE:devvm000.lla0.facebook.com",
    "MACHINE_TIER:devvm",
    "USER:myusername0"
  ]
