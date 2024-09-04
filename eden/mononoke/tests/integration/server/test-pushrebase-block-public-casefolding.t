# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_mononoke_config
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="main"
  > [[bookmarks]]
  > regex=".*"
  > hooks_skip_ancestors_of=["main"]
  > CONFIG

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:run_hooks_on_additional_changesets": true
  >   }
  > }
  > EOF

  $ setup_common_hg_configs
  $ cd $TESTTMP

  $ configure dummyssh
  $ enable amend

setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ drawdag <<EOF
  > B C  # C/existing/caseconflict = caseconflict
  > |/   # C/existing/CaseConflict = caseconflict
  > A    # A/somefile = somefile
  > EOF

  $ hg bookmark main -r $B
  $ hg bookmark other -r $C

blobimport
  $ cd ..
  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
clone
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ enable pushrebase remotenames infinitepush commitcloud
  $ setconfig infinitepush.server=false infinitepush.branchpattern='re:scratch/.+'

attempt to push a case conflict onto main
  $ hg up -q main
  $ echo caseconflict > caseconflict.txt
  $ echo caseconflict > CaseConflict.txt
  $ hg add caseconflict.txt CaseConflict.txt
  warning: possible case-folding collision for caseconflict.txt
  $ hg commit -qm conflict1
  $ hg push -r . --to main
  pushing rev ddbe318d5aca to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (8c2a70bb0c78, ddbe318d5aca] (1 commit) to remote bookmark main
  abort: Server error: invalid request: Case conflict found in faaf8134514581fac83a1feaf35cee8ece18561a89bcac7e2be927395465938a: CaseConflict.txt conflicts with caseconflict.txt
  [255]

it's ok to push it on to a scratch bookmark, though
  $ hg push -r . --to scratch/conflict1 --create
  pushing to mono:repo
  searching for changes

if we stack a commit that fixes the case conflict, we still can't land the stack
  $ hg rm caseconflict.txt
  $ hg commit -qm "fix conflict"
  $ hg push -r . --to main
  pushing rev cbb97717004c to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (8c2a70bb0c78, cbb97717004c] (2 commits) to remote bookmark main
  abort: Server error: invalid request: Case conflict found in faaf8134514581fac83a1feaf35cee8ece18561a89bcac7e2be927395465938a: CaseConflict.txt conflicts with caseconflict.txt
  [255]

attempt to push a commit that introduces a case conflict onto main
  $ hg up -q main
  $ echo caseconflict > SomeFile
  $ hg add SomeFile
  warning: possible case-folding collision for SomeFile
  $ hg commit -qm conflict2
  $ hg push -r . --to main
  pushing rev 99950f688a32 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (8c2a70bb0c78, 99950f688a32] (1 commit) to remote bookmark main
  abort: Server error: invalid request: Case conflict found in 273fd0b40d61b2582af82625cbd3d60f2c35f4e5ec819191f4f3a7adbc21dec2: SomeFile conflicts with somefile
  [255]

again, it's ok to push this to a scratch branch
  $ hg push -r . --to scratch/conflict2 --create
  pushing to mono:repo
  searching for changes

we can move the bookmark to a commit with a pre-existing case conflict via bookmark-only pushrebase
  $ hg push -r other --to main --pushvar NON_FAST_FORWARD=true
  pushing rev 2b2f2fedc926 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark main
  moving remote bookmark main from 8c2a70bb0c78 to 2b2f2fedc926

we can't land to the other if we introduce a new case conflict
  $ hg up -q other
  $ echo conflict > testfile
  $ echo conflict > TestFile
  $ hg add testfile TestFile
  warning: possible case-folding collision for testfile
  $ hg commit -qm conflict3
  $ hg push -r . --to other
  pushing rev 379371c4bd8a to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (2b2f2fedc926, 379371c4bd8a] (1 commit) to remote bookmark other
  abort: Server error: invalid request: Case conflict found in a1b0639259bc3524b3d1db9b85b9300b1fb9f17c0c60d39e0bd64efa879c5dd5: TestFile conflicts with testfile
  [255]

we can land something that doesn't introduce a new case conflict
  $ hg hide -q .
  $ echo testfile > testfile
  $ hg add testfile
  $ hg commit -qm nonewconflict
  $ hg push -r . --to other
  pushing rev 951a1a92f401 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (2b2f2fedc926, 951a1a92f401] (1 commit) to remote bookmark other
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark other to 951a1a92f401

we can't land if we try to make an existing case conflict worse
  $ echo conflict > existing/CASECONFLICT
  $ hg add existing/CASECONFLICT
  warning: possible case-folding collision for existing/CASECONFLICT
  $ hg commit -qm conflict4
  $ hg push -r . --to other
  pushing rev 13488940ae4f to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (951a1a92f401, 13488940ae4f] (1 commit) to remote bookmark other
  abort: Server error: invalid request: Case conflict found in b6801c5486aaa96f45805ddd8c874a5e602888e94cc2c93e44aacdc106e8ed9d: existing/CASECONFLICT conflicts with existing/CaseConflict
  [255]

we can land it if we also fix all of the related case conflicts
  $ hg rm existing/CaseConflict
  $ hg rm existing/caseconflict
  $ hg amend -q
  $ hg push -r . --to other
  pushing rev f53c362f9b2d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (951a1a92f401, f53c362f9b2d] (1 commit) to remote bookmark other
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark other to f53c362f9b2d
