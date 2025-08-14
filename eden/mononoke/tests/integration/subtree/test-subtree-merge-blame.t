# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setconfig push.edenapi=true
  $ setconfig subtree.min-path-depth=1
  $ setconfig subtree.use-prod-subtree-key=True
  $ setconfig experimental.edenapi-blame=True
  $ enable amend
  $ setup_common_config

  $ testtool_drawdag -R repo --derive-all --no-default-files << EOF
  > A-B
  > # modify: A foo/file1 "aaa\nbbb\nccc\n"
  > # modify: B other "1\n"
  > # bookmark: B master_bookmark
  > EOF
  A=5fd0cad9f3006528cffc46cb5a228c1ef9186787d769a355de1b7ee953e0ceca
  B=5791bd68efe5abf5ffd2637517895f5d6a672b077349b4996898d82de18a1834

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo 

  $ hg subtree copy -r .^ --from-path foo --to-path bar
  copying foo to bar
  $ cat > bar/file1 <<EOF
  > aaa
  > mmm
  > nnn
  > ccc
  > EOF
  $ hg amend
  $ echo 2 >> other
  $ hg commit -qm "other 2"
  $ hg push -r . --to master_bookmark
  pushing rev aa7c579b9d86 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 3 trees for upload
  edenapi: uploaded 3 trees
  edenapi: uploaded 2 changesets
  pushrebasing stack (e060a821ce0c, aa7c579b9d86] (2 commits) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to aa7c579b9d86

  $ hg subtree copy -r .^ --from-path bar --to-path baz
  copying bar to baz
  $ cat > baz/file1 <<EOF
  > aaa
  > mmm
  > ooo
  > ccc
  > EOF
  $ hg amend
  $ echo 3 >> other
  $ hg commit -qm "other 3"
  $ cat > baz/file1 <<EOF
  > aaa
  > mmm
  > xxx
  > ooo
  > ccc
  > EOF
  $ hg commit -qm "X"
  $ echo 4 >> other
  $ hg commit -qm "other 3"
  $ hg push -r . --to master_bookmark
  pushing rev a707b7c52df8 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 4 commits for upload
  edenapi: queue 4 files for upload
  edenapi: uploaded 4 files
  edenapi: queue 6 trees for upload
  edenapi: uploaded 6 trees
  edenapi: uploaded 4 changesets
  pushrebasing stack (aa7c579b9d86, a707b7c52df8] (4 commits) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to a707b7c52df8
  $ hg log -G -T '{node|short} {desc}\n'
  @  a707b7c52df8 other 3
  │
  o  62edd74cba5e X
  │
  o  e544a4043c11 other 3
  │
  o  4f70903af235 Subtree copy from e014daf07274e66064fc09aa2bb46ed16b175323
  │  - Copied path bar to baz
  o  aa7c579b9d86 other 2
  │
  o  e014daf07274 Subtree copy from 8cdb209b9dcde94c52dfb9a4d2f003d4198a17cf
  │  - Copied path foo to bar
  o  e060a821ce0c B
  │
  o  8cdb209b9dcd A
  
 
Create a complex merge that includes lines that were copied from a path that is not in the destination history.
  $ hg subtree merge -r .^ --from-path baz --to-path foo
  searching for merge base ...
  found the last subtree copy commit e014daf07274
  merge base: 8cdb209b9dcd
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg diff
  diff -r a707b7c52df8 foo/file1
  --- a/foo/file1	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foo/file1	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,3 +1,5 @@
   aaa
  -bbb
  +mmm
  +xxx
  +ooo
   ccc
  $ cat > foo/file1 <<EOF
  > aaa
  > ppp
  > xxx
  > ooo
  > ccc
  > EOF
  $ hg commit
  $ hg push -r . --to master_bookmark
  pushing rev 01009ac7e678 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (a707b7c52df8, 01009ac7e678] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 01009ac7e678
  $ merge=$(hg whereami)

  $ mononoke_admin derived-data -R repo derive -T blame -i $merge
