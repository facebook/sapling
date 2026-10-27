# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Grant client0 full read+write access and client2 read-only access. client1 has
no access at all.
  $ cat >> "$ACL_FILE" << ACLS
  > {
  >   "repos": {
  >     "default": {
  >       "actions": {
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "$CLIENT2_ID_TYPE:$CLIENT2_ID_DATA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"]
  >       }
  >     }
  >   }
  > }
  > ACLS

  $ setup_common_config
  $ cd $TESTTMP
  $ testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  $ start_and_wait_for_mononoke_server

  $ hg clone -q mono:repo repo
  $ cd repo
  $ setconfig push.edenapi=true
  $ echo m > m && hg addremove -q && hg ci -qm m

  $ NO_ACCESS="--config auth.mononoke.cert=$TEST_CERTDIR/client1.crt --config auth.mononoke.key=$TEST_CERTDIR/client1.key"
  $ READ_ONLY="--config auth.mononoke.cert=$TEST_CERTDIR/client2.crt --config auth.mononoke.key=$TEST_CERTDIR/client2.key"

SLAPI denies an identity without read access with HTTP 403. Sapling reports the
server's reason and a hint, whether or not the network doctor is enabled:
  $ hg pull $NO_ACCESS
  pulling from mono:repo
  abort: permission denied: Failed to load repository: repo: permission denied: Repo metadata read access in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]
  $ hg pull $NO_ACCESS --config experimental.network-doctor=true
  pulling from mono:repo
  abort: permission denied: Failed to load repository: repo: permission denied: Repo metadata read access in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]
  $ hg push --to master_bookmark $NO_ACCESS
  abort: permission denied: Failed to load repository: repo: permission denied: Repo metadata read access in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]

The same applies when the first denied request is a push's upload:
  $ hg push --to master_bookmark $NO_ACCESS --config push.use_local_bookmark_value=true
  pushing rev 0d40ce7c3acb to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  abort: permission denied: Failed to load repository: repo: permission denied: Repo metadata read access in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]

Code that turns a transport error into its own abort leaves a denial alone:
  $ hg debugapi $NO_ACCESS -e bookmarks -i "['master_bookmark']"
  abort: permission denied: Failed to load repository: repo: permission denied: Repo metadata read access in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]

Listing remote bookmarks and looking up a remote commit use the wire protocol:
  $ hg bookmarks --remote $NO_ACCESS
  remote: Authorization failed:
  remote:   Error:
  remote:     Unauthorized access, permission denied
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hg id -r master_bookmark mono:repo $NO_ACCESS
  remote: Authorization failed:
  remote:   Error:
  remote:     Unauthorized access, permission denied
  abort: unexpected EOL, expected netstring digit
  [255]

A read-only identity can upload commits but not move or create a public
bookmark:
  $ hg push --to master_bookmark $READ_ONLY
  pushing rev 0d40ce7c3acb to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (20ca2a4749a4, 0d40ce7c3acb] (1 commit) to remote bookmark master_bookmark
  abort: Server error: permission denied: Repo write access for LandStack(Publishing) in repo 'repo' is not permitted with Identity for * (glob)
  [255]
  $ hg push --to master_bookmark --force $READ_ONLY
  pushing rev 0d40ce7c3acb to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  moving remote bookmark master_bookmark from 20ca2a4749a4 to 0d40ce7c3acb
  abort: server error: permission denied: Repo write access for UpdateBookmark(Publishing) in repo 'repo' is not permitted with Identity for * (glob)
  [255]
  $ hg push --to newbook --create $READ_ONLY
  pushing rev 0d40ce7c3acb to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark newbook
  creating remote bookmark newbook
  abort: failed to create remote bookmark:
    remote server error: permission denied: Repo write access for CreateBookmark(Publishing) in repo 'repo' is not permitted with Identity for * (glob)
  [255]

The server marks a write that access control denied with error code 4
(`PermissionDenied`), for both a bookmark move and a land:
  $ HEAD=$(hg log -r . -T '{node}')
  $ BASE=$(hg log -r master_bookmark -T '{node}')
  $ hg debugapi $READ_ONLY -e setbookmark -i "'master_bookmark'" -i "'$HEAD'" -i "'$BASE'"
  {"data": {"Err": {"code": 4,
                    "message": "permission denied: Repo write access for UpdateBookmark(Publishing) in repo 'repo' is not permitted with Identity for *"}}} (glob)
  $ hg debugapi $READ_ONLY -e landstack -i "'master_bookmark'" -i "'$HEAD'" -i "'$BASE'"
  {"data": {"Err": {"code": 4,
                    "message": "permission denied: Repo write access for LandStack(Publishing) in repo 'repo' is not permitted with Identity for *"}}} (glob)

Commit cloud reports a denial in the same form. Without local backup state, a
backup first asks the server which commits it already has:
  $ setconfig extensions.commitcloud= commitcloud.hostname=testhost commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ hg cloud upload $NO_ACCESS
  abort: permission denied: Failed to load repository: repo: permission denied: Repo metadata read access in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]

A forced backup skips that check, and does not retry the denied upload:
  $ hg cloud upload -q
  $ hg cloud backup --force $NO_ACCESS
  commitcloud: head '0d40ce7c3acb' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  abort: permission denied: Failed to load repository: repo: permission denied: Repo metadata read access in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]

A sync uploads new commits before anything else:
  $ hg cloud join -q
  $ echo n > n && hg addremove -q && hg ci -qm n
  $ hg cloud sync $NO_ACCESS
  commitcloud: synchronizing 'repo' with 'user/test/default'
  abort: permission denied: Failed to load repository: repo: permission denied: Repo metadata read access in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]

So does listing remote bookmarks by pattern:
  $ hg bookmarks --list-remote 'master*' $NO_ACCESS
  abort: permission denied: Failed to load repository: repo: permission denied: Repo metadata read access in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]
