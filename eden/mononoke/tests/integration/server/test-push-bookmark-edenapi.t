# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
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

clone the repo. EdenApi is on (realistic prod), but the -B bookmark
killswitch push.edenapi-bookmark is left at its default-off value, so -B
bookmark pushes still go through the legacy wireproto path.
  $ hg clone -q mono:repo repo-push
  $ cd repo-push
  $ setconfig push.edenapi=true

Create a bookmark with -B. This documents the current wireproto behavior:
the push goes to the mono:repo destination and the bookmark is exported over
the wire protocol.
  $ echo foo > foo && hg addremove -q && hg ci -qm foo
  $ hg bookmark foo
  $ hg push -B foo --create
  pushing to mono:repo
  searching for changes
  exporting bookmark foo

Fast-forward the bookmark with -B. Again this goes over wireproto and the
bookmark is updated in place.
  $ echo foo2 > foo && hg ci -qm foo2
  $ hg push -B foo
  pushing to mono:repo
  searching for changes
  updating bookmark foo

Pushvars propagate from the client to the server. Make a divergent commit and
point foo at it (a non-fast-forward move). The server rejects the move by
default, but accepts the identical push when NON_FAST_FORWARD=true is sent as a
pushvar -- the only difference between the two pushes -- proving the pushvar
reaches the server and is honored on the bookmark-movement path (shared by the
wireproto and EdenApi -B paths).
  $ hg update -q foo^
  $ echo div > div && hg addremove -q && hg ci -qm divergent
  $ hg bookmark -r . -f foo
  $ hg push -B foo --non-forward-move
  pushing to mono:repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:     Caused by:
  remote:         0: Failed to fast-forward bookmark (set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)
  remote:         1: Non fast-forward bookmark move of 'foo' from * to * (glob)
  abort: unexpected EOL, expected netstring digit
  [255]
The same push with NON_FAST_FORWARD=true succeeds. The divergent commit was
already uploaded by the rejected attempt above, so no changesets are sent here
(hence "no changes found" and exit 1); the point is that the bookmark now moves.
  $ hg push -B foo --non-forward-move --pushvar NON_FAST_FORWARD=true
  pushing to mono:repo
  searching for changes
  no changes found
  updating bookmark foo
  [1]

Repo access control is enforced for bookmark pushes, before the push is
processed and on both the wireproto and EdenApi paths. An identity in the repo
ACL (the default client0 cert) can move master_bookmark with -B, but an identity
that is not in the repo ACL is rejected. (This uses the repo ACL rather than a
per-bookmark allowed_users restriction because allowed_users matches the USER
unix_name identity, which is not available in the OSS/Sandcastle test environment
-- there the client cert only carries an X509_SUBJECT_NAME identity.)

Authorized identity (default client0) moves master_bookmark with -B (a
fast-forward):
  $ hg update -q master_bookmark
  $ echo m > m && hg addremove -q && hg ci -qm m
  $ hg bookmark master_bookmark
  $ hg push -B master_bookmark
  pushing to mono:repo
  searching for changes
  updating bookmark master_bookmark

An identity not in the repo ACL (client1) cannot push to master_bookmark:
  $ echo m2 > m2 && hg addremove -q && hg ci -qm m2
  $ hg push -B master_bookmark --config auth.mononoke.cert="$TEST_CERTDIR/client1.crt" --config auth.mononoke.key="$TEST_CERTDIR/client1.key"
  pushing to mono:repo
  remote: Authorization failed:
  remote:   Error:
  remote:     Unauthorized access, permission denied
  abort: unexpected EOL, expected netstring digit
  [255]
