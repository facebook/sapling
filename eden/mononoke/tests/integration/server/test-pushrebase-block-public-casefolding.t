# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_mononoke_config
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="master_bookmark"
  > [[bookmarks]]
  > regex=".*"
  > hooks_skip_ancestors_of=["master_bookmark"]
  > CONFIG

  $ setup_common_hg_configs
  $ setconfig remotenames.selectivepulldefault=master_bookmark,other
  $ cd $TESTTMP

  $ configure dummyssh
  $ enable amend

setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo <<EOF
  > A-B
  > A-C
  > # modify: A "somefile" "somefile\n"
  > # modify: C "existing/caseconflict" "caseconflict\n"
  > # modify: C "existing/CaseConflict" "caseconflict\n"
  > # bookmark: B master_bookmark
  > # bookmark: C other
  > EOF
  A=17e539a4e3bfe31553dff43e1f990edf4403d4f8ec1a056fa9267d6b91263a48
  B=8108bb23c941f8af782360c3554489f9bfea96b3bcf1473eeba1c810690a2ba6
  C=fc6bd7272646f0fb63ea9f78f055c9493cc2e841dce663474ff1384e8cdecb95

Import and start mononoke
  $ cd "$TESTTMP"
  $ mononoke
  $ wait_for_mononoke
clone
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ enable pushrebase commitcloud
  $ setconfig infinitepush.server=false infinitepush.branchpattern='re:scratch/.+'

attempt to push a case conflict onto master_bookmark
  $ hg up -q master_bookmark
  $ echo caseconflict > caseconflict.txt
  $ echo caseconflict > CaseConflict.txt
  $ hg add caseconflict.txt CaseConflict.txt
  warning: possible case-folding collision for caseconflict.txt
  $ hg commit -qm conflict1
  $ hg push -r . --to master_bookmark
  pushing rev b32b52979419 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (6a5618159aea, b32b52979419] (1 commit) to remote bookmark master_bookmark
  abort: Server error: invalid request: Case conflict found in 97100e2cbeb3b99a7fdc9090a45f81cf643f214a6791d06db652d1a33499059d: CaseConflict.txt conflicts with caseconflict.txt
  [255]

it's ok to push it on to a scratch bookmark, though
  $ hg push -qr . --to scratch/conflict1 --create

if we stack a commit that fixes the case conflict, we still can't land the stack
  $ hg rm caseconflict.txt
  $ hg commit -qm "fix conflict"
  $ hg push -r . --to master_bookmark
  pushing rev 9605865ae845 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (6a5618159aea, 9605865ae845] (2 commits) to remote bookmark master_bookmark
  abort: Server error: invalid request: Case conflict found in 97100e2cbeb3b99a7fdc9090a45f81cf643f214a6791d06db652d1a33499059d: CaseConflict.txt conflicts with caseconflict.txt
  [255]

attempt to push a commit that introduces a case conflict onto master_bookmark
  $ hg up -q master_bookmark
  $ echo caseconflict > SomeFile
  $ hg add SomeFile
  warning: possible case-folding collision for SomeFile
  $ hg commit -qm conflict2
  $ hg push -r . --to master_bookmark
  pushing rev 3d2ddf00b317 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (6a5618159aea, 3d2ddf00b317] (1 commit) to remote bookmark master_bookmark
  abort: Server error: invalid request: Case conflict found in 6dce7b1c2ec1493b75259e9a831d9afaa98906e05281d3bb95414817ef858077: SomeFile conflicts with somefile
  [255]

again, it's ok to push this to a scratch branch
  $ hg push -qr . --to scratch/conflict2 --create

we can move the bookmark to a commit with a pre-existing case conflict via bookmark-only pushrebase
  $ hg push -r other --to master_bookmark --pushvar NON_FAST_FORWARD=true
  pushing rev e43e4176109c to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  moving remote bookmark master_bookmark from 6a5618159aea to e43e4176109c

we can't land to the other if we introduce a new case conflict
  $ hg up -q other
  $ echo conflict > testfile
  $ echo conflict > TestFile
  $ hg add testfile TestFile
  warning: possible case-folding collision for testfile
  $ hg commit -qm conflict3
  $ hg push -r . --to other
  pushing rev aa441d3eb20d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (e43e4176109c, aa441d3eb20d] (1 commit) to remote bookmark other
  abort: Server error: invalid request: Case conflict found in ee3c0c0dd1a4b8eae1c7086a6673d0938ca1a47e67542fa173aa139632ec9ba8: TestFile conflicts with testfile
  [255]

we can land something that doesn't introduce a new case conflict
  $ hg hide -q .
  $ echo testfile > testfile
  $ hg add testfile
  $ hg commit -qm nonewconflict
  $ hg push -r . --to other
  pushing rev 7af5d4a84c55 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (e43e4176109c, 7af5d4a84c55] (1 commit) to remote bookmark other
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark other to 7af5d4a84c55

We can land adding a new file that makes an existing case conflict worse
  $ echo conflict > existing/CASECONFLICT
  $ hg add existing/CASECONFLICT
  warning: possible case-folding collision for existing/CASECONFLICT
  $ hg commit -qm conflict4
  $ hg push -r . --to other
  pushing rev 38d9cb4e81a0 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (7af5d4a84c55, 38d9cb4e81a0] (1 commit) to remote bookmark other
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark other to 38d9cb4e81a0
