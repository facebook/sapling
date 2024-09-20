# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup a Mononoke repo.

  $ LFS_THRESHOLD="10" setup_common_config "blob_files"
  $ cd "$TESTTMP"

Start Mononoke & LFS. Require TLS certs in LFS.

  $ start_and_wait_for_mononoke_server
  $ lfs_url="$(lfs_server --tls --scuba-dataset "file://$TESTTMP/scuba.json")/repo"

Create a repo

  $ hg clone -q mono:repo repo
  $ cd repo
  $ yes 2>/dev/null | head -c 100 > large
  $ hg add large
  $ hg ci -ma
  $ hg push -q --to master --create
  $ cd "$TESTTMP"

Clone the repo. Enable LFS. Take a different cache path to make sure we have to go to the server.

  $ hg clone -q mono:repo repo-clone --noupdate
  $ cd repo-clone
  $ setup_hg_modern_lfs "$lfs_url" 10B
  $ setconfig "remotefilelog.cachepath=$TESTTMP/cachepath2"

Initially, unconfigure client certs. This will fail, because certs are required.

  $ hg up master -q --config auth.mononoke.schemes=doesntmatch 2>&1 | grep -i 'certificate' -m 1
  tls error: [60] SSL peer certificate or SSH remote key was not OK (SSL certificate problem: self?signed certificate in certificate chain)! (glob)
  $ ! test -f large

Now, with certs. This will work.

  $ hg up master -q
  $ test -f large

Finally, check what identities the client presented.

  $ wait_for_json_record_count "$TESTTMP/scuba.json" 2
  $ diff <(
  >   jq -S .normvector.client_identities "$TESTTMP/scuba.json"
  > ) <(
  >   printf "$JSON_CLIENT_ID\n$JSON_CLIENT_ID" | jq -S .
  > )
