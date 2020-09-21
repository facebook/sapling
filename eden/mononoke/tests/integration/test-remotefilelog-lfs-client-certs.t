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

Initially, enable the killswitch This will fail, because we don't have certs.

  $ setconfig "lfs.use-client-certs=false"
  $ hgmn up master -q 2>&1 | grep -i 'ssl'
  * SSL peer certificate or SSH remote key was not OK * (glob)
  $ ! test -f large

Now, remove the killswitch. This will work

  $ setconfig "lfs.use-client-certs=true"
  $ hgmn up master -q
  $ test -f large

Finally, check what identities the client presented.

  $ wait_for_json_record_count "$TESTTMP/scuba.json" 2
  $ diff <(
  >   jq -S .normvector.client_identities "$TESTTMP/scuba.json"
  > ) <(
  >   printf "$JSON_CLIENT_ID\n$JSON_CLIENT_ID" | jq -S .
  > )
