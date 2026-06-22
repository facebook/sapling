# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Grant client0 full read+write access and client2 read-only access, so we can
show the wireproto ACL gap on the bare-push path: an identity with no access
(client1) is rejected, while an identity that can read but not write (client2)
still moves master_bookmark because the wireproto repo-write check is downgraded
to log-only.
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

setup configuration
  $ export ONLY_FAST_FORWARD_BOOKMARK="master_bookmark"
  $ setup_common_config
  $ cd $TESTTMP

setup repo

  $ testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

start mononoke
  $ start_and_wait_for_mononoke_server

clone the repo. EdenApi is on (realistic prod) but the bookmark killswitch
push.edenapi-bookmark is NOT enabled, so the bare-push fast-forward goes over
the legacy wireproto path (the advsrc branch of exchange._pushdiscoverybookmarks).
  $ hg clone -q mono:repo repo-push
  $ cd repo-push
  $ setconfig push.edenapi=true
  $ setconfig remotenames.pushrev=.

Create a LOCAL bookmark whose name matches the remote pull-default bookmark
(master_bookmark), then make a new commit so the local bookmark fast-forwards
ahead of its remote namesake.
  $ hg up -q master_bookmark
  $ hg bookmark master_bookmark
  $ echo foo > foo && hg addremove -q && hg ci -qm foo

A bare push (no -B, no --to) auto-moves the local bookmark that has
fast-forwarded vs its remote namesake. listkeys("bookmarks") returns the
pull-default master_bookmark, so the advsrc branch performs the move over
wireproto.
  $ hg push
  pushing to mono:repo
  searching for changes
  updating bookmark master_bookmark

Repo access control on the bare-push path, before the push is processed and on
both the wireproto and EdenApi paths. The bare push above used the default
client0 cert (full access) and moved master_bookmark.

An identity with no access (client1) cannot move master_bookmark with a bare
push:
  $ echo foo2 > foo && hg ci -qm foo2
  $ hg push --config auth.mononoke.cert="$TEST_CERTDIR/client1.crt" --config auth.mononoke.key="$TEST_CERTDIR/client1.key"
  pushing to mono:repo
  remote: Authorization failed:
  remote:   Error:
  remote:     Unauthorized access, permission denied
  abort: unexpected EOL, expected netstring digit
  [255]

An identity with read but not write access (client2) still moves master_bookmark
over wireproto -- the bare-push ACL gap. The wireproto repo-write check is
downgraded to log-only, so the move is allowed (whereas the EdenApi path denies
it at the write):
  $ echo foo3 > foo && hg ci -qm foo3
  $ hg push --config auth.mononoke.cert="$TEST_CERTDIR/client2.crt" --config auth.mononoke.key="$TEST_CERTDIR/client2.key"
  pushing to mono:repo
  searching for changes
  updating bookmark master_bookmark
