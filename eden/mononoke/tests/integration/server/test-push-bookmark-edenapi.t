# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Grant client0 full read+write access and client2 read-only access, so we can
exercise the ACL behavior on the -B push. An identity with no access at all
(client1) is rejected, and an identity that can read but not write (client2)
is rejected at the bookmark write. (Over wireproto the client2 push succeeds,
because the wireproto repo-write check is downgraded to log-only.)
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
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ cd $TESTTMP

setup repo

  $ testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

start mononoke
  $ start_and_wait_for_mononoke_server

clone the repo. EdenApi is on (realistic prod) and experimental.slapi-bookmark-push
is enabled, so `-B BOOKMARK` is pushed as `-r BOOKMARK --to BOOKMARK --create
--no-rebase` through EdenApi instead of over the wire protocol.
  $ hg clone -q mono:repo repo-push
  $ cd repo-push
  $ setconfig push.edenapi=true
  $ setconfig experimental.slapi-bookmark-push=true

Create a bookmark with -B. The commit is uploaded and the remote bookmark is
created at it.
  $ echo foo > foo && hg addremove -q && hg ci -qm foo
  $ hg bookmark foo
  $ hg push -B foo --create
  pushing rev 8a6cbbc85c36 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark foo
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  creating remote bookmark foo

Fast-forward the bookmark with -B. The remote bookmark moves to exactly the
local bookmark.
  $ echo foo2 > foo && hg ci -qm foo2
  $ hg push -B foo
  pushing rev cecde540a147 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark foo
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  moving remote bookmark foo from 8a6cbbc85c36 to cecde540a147

Pushing again when the remote bookmark already points at the local bookmark
is a no-op, and still exits 0:
  $ hg push -B foo
  pushing rev cecde540a147 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark foo
  moving remote bookmark foo from cecde540a147 to cecde540a147

Make a divergent commit and point foo at it (a non-fast-forward move). The
move is rejected by default, but accepted when NON_FAST_FORWARD=true is sent
as a pushvar -- the only difference between the two pushes.
  $ hg update -q foo^
  $ echo div > div && hg addremove -q && hg ci -qm divergent
  $ hg bookmark -r . -f foo
  $ hg push -B foo --non-forward-move
  pushing rev a4c79565beb5 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark foo
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  abort: non-fast-forward push to remote bookmark foo from cecde540a147 to a4c79565beb5
  (add '--force' or set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)
  [255]
The same push with NON_FAST_FORWARD=true succeeds. The divergent commit was
already uploaded by the rejected attempt above, so only the bookmark moves.
  $ hg push -B foo --non-forward-move --pushvar NON_FAST_FORWARD=true
  pushing rev a4c79565beb5 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark foo
  moving remote bookmark foo from cecde540a147 to a4c79565beb5

--force also allows a non-fast-forward move:
  $ hg update -q foo^
  $ echo div2 > div && hg addremove -q && hg ci -qm divergent2
  $ hg bookmark -r . -f foo
  $ hg push -B foo --force
  pushing rev 08332e0eaa63 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark foo
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  moving remote bookmark foo from a4c79565beb5 to 08332e0eaa63

-B pushes a local bookmark. Without one it aborts, rather than deleting the
remote bookmark:
  $ hg push -B nonexistent
  abort: local bookmark 'nonexistent' does not exist
  (use 'hg push --delete nonexistent' to delete the remote bookmark)
  [255]

-B pushes exactly one bookmark, and takes no other destination or commit:
  $ hg push -B foo -B master_bookmark
  abort: only one --bookmark/-B can be pushed at a time
  [255]
  $ hg push -B foo -r .
  abort: do not specify --rev/-r and --bookmark/-B at the same time
  [255]
  $ hg push -B foo --to foo
  abort: do not specify --to/-t and --bookmark/-B at the same time
  [255]
  $ hg push -B foo --no-rebase
  abort: --no-rebase requires --to
  [255]

Without SLAPI push (push.edenapi=false), -B takes the wireproto push:
  $ hg update -q foo
  $ echo div3 > div && hg ci -qm divergent3
  $ hg push -B foo --config push.edenapi=false
  pushing to mono:repo
  searching for changes
  updating bookmark foo

EdenApi push does not support merge commits yet, so -B falls back to the
wireproto push for them:
  $ hg update -q master_bookmark
  $ echo side > side && hg addremove -q && hg ci -qm side
  $ hg merge -q foo
  $ hg ci -qm merge
  $ hg bookmark -f foo
  $ hg push -B foo
  fallback reason: merge commit is not supported by EdenApi push yet
  pushing to mono:repo
  searching for changes
  updating bookmark foo

Repo access control is enforced for bookmark pushes. An identity in the repo
ACL (the default client0 cert) can move master_bookmark with -B, but an
identity that is not in the repo ACL is rejected. (This uses the repo ACL
rather than a per-bookmark allowed_users restriction because allowed_users
matches the USER unix_name identity, which is not available in the
OSS/Sandcastle test environment -- there the client cert only carries an
X509_SUBJECT_NAME identity.)

Authorized identity (default client0) moves master_bookmark with -B (a
fast-forward):
  $ hg update -q master_bookmark
  $ echo m > m && hg addremove -q && hg ci -qm m
  $ hg bookmark master_bookmark
  $ hg push -B master_bookmark
  pushing rev 0d40ce7c3acb to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  moving remote bookmark master_bookmark from 20ca2a4749a4 to 0d40ce7c3acb

An identity not in the repo ACL (client1) cannot push to master_bookmark:
  $ echo m2 > m2 && hg addremove -q && hg ci -qm m2
  $ hg push -B master_bookmark --config auth.mononoke.cert="$TEST_CERTDIR/client1.crt" --config auth.mononoke.key="$TEST_CERTDIR/client1.key"
  abort: permission denied: Failed to load repository: repo: permission denied: Repo metadata read access in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]

An identity with read but not write access (client2) is denied at the
bookmark write. Automation that uses -B needs repo write access.
  $ echo m3 > m3 && hg addremove -q && hg ci -qm m3
  $ hg push -B master_bookmark --config auth.mononoke.cert="$TEST_CERTDIR/client2.crt" --config auth.mononoke.key="$TEST_CERTDIR/client2.key"
  pushing rev 7dae027ab582 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  moving remote bookmark master_bookmark from 0d40ce7c3acb to 7dae027ab582
  abort: permission denied: Repo write access for UpdateBookmark(Publishing) in repo 'repo' is not permitted with Identity for * (glob)
  (check that you have access to this repository)
  [255]

-B pushes a scratch bookmark as it pushes any other bookmark. With SLAPI push,
that is a plain SLAPI push to the remote bookmark of the same name:
  $ setconfig infinitepush.branchpattern="re:scratch/.+"
  $ hg bookmark -r . scratch/fromdot
  $ hg push -B scratch/fromdot
  pushing rev 7dae027ab582 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark scratch/fromdot
  creating remote bookmark scratch/fromdot

Without SLAPI push, it is the wireproto -B push, which Mononoke rejects for a
scratch bookmark:
  $ hg bookmark -r .^ scratch/fromparent
  $ hg push -B scratch/fromparent --config push.edenapi=false
  pushing to mono:repo
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:     Caused by:
  remote:         0: Failed to create bookmark
  remote:         1: Invalid publishing bookmark: scratch/fromparent (only scratch bookmarks may match pattern ^scratch/.+$)
  abort: unexpected EOL, expected netstring digit
  [255]
