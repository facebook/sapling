# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_mononoke_config
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="main"
  > [[bookmarks]]
  > regex=".*"
  > hooks_skip_ancestors_of=["main"]
  > CONFIG

  $ cat > $TESTTMP/mononoke_tunables.json <<EOF
  > {
  >   "killswitches": {
  >     "check_case_conflicts_on_bookmark_movement": true,
  >     "skip_case_conflict_check_on_changeset_upload": true,
  >     "run_hooks_on_additional_changesets": true
  >   }
  > }
  > EOF

  $ setup_common_hg_configs
  $ cd $TESTTMP

  $ configure dummyssh
  $ enable amend

setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ drawdag <<EOF
  > B C  # C/existing/caseconflict = caseconflict
  > |/   # C/existing/CaseConflict = caseconflict
  > A    # A/somefile = somefile
  > EOF

  $ hg bookmark main -r $B
  $ hg bookmark other -r $C

blobimport
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke

clone
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2
  $ setup_hg_client
  $ enable pushrebase remotenames infinitepush commitcloud
  $ setconfig infinitepush.server=false infinitepush.branchpattern='re:scratch/.+'

attempt to push a case conflict onto main
  $ hg up -q main
  $ echo caseconflict > caseconflict.txt
  $ echo caseconflict > CaseConflict.txt
  $ hg add caseconflict.txt CaseConflict.txt
  warning: possible case-folding collision for caseconflict.txt
  $ hg commit -qm conflict1
  $ hgmn push -r . --to main
  pushing rev ddbe318d5aca to destination ssh://user@dummy/repo bookmark main
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Case conflict found in faaf8134514581fac83a1feaf35cee8ece18561a89bcac7e2be927395465938a: CaseConflict.txt conflicts with caseconflict.txt
  remote: 
  remote:   Root cause:
  remote:     Case conflict found in faaf8134514581fac83a1feaf35cee8ece18561a89bcac7e2be927395465938a: CaseConflict.txt conflicts with caseconflict.txt
  remote: 
  remote:   Debug context:
  remote:     CaseConflict {
  remote:         changeset_id: ChangesetId(
  remote:             Blake2(faaf8134514581fac83a1feaf35cee8ece18561a89bcac7e2be927395465938a),
  remote:         ),
  remote:         path1: MPath("CaseConflict.txt"),
  remote:         path2: MPath("caseconflict.txt"),
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

it's ok to push it on to a scratch bookmark, though
  $ hgmn push -r . --to scratch/conflict1 --create
  pushing to ssh://user@dummy/repo
  searching for changes

if we stack a commit that fixes the case conflict, we still can't land the stack
  $ hg rm caseconflict.txt
  $ hg commit -qm "fix conflict"
  $ hgmn push -r . --to main
  pushing rev cbb97717004c to destination ssh://user@dummy/repo bookmark main
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Case conflict found in faaf8134514581fac83a1feaf35cee8ece18561a89bcac7e2be927395465938a: CaseConflict.txt conflicts with caseconflict.txt
  remote: 
  remote:   Root cause:
  remote:     Case conflict found in faaf8134514581fac83a1feaf35cee8ece18561a89bcac7e2be927395465938a: CaseConflict.txt conflicts with caseconflict.txt
  remote: 
  remote:   Debug context:
  remote:     CaseConflict {
  remote:         changeset_id: ChangesetId(
  remote:             Blake2(faaf8134514581fac83a1feaf35cee8ece18561a89bcac7e2be927395465938a),
  remote:         ),
  remote:         path1: MPath("CaseConflict.txt"),
  remote:         path2: MPath("caseconflict.txt"),
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

attempt to push a commit that introduces a case conflict onto main
  $ hg up -q main
  $ echo caseconflict > SomeFile
  $ hg add SomeFile
  warning: possible case-folding collision for SomeFile
  $ hg commit -qm conflict2
  $ hgmn push -r . --to main
  pushing rev 99950f688a32 to destination ssh://user@dummy/repo bookmark main
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Case conflict found in 273fd0b40d61b2582af82625cbd3d60f2c35f4e5ec819191f4f3a7adbc21dec2: SomeFile conflicts with somefile
  remote: 
  remote:   Root cause:
  remote:     Case conflict found in 273fd0b40d61b2582af82625cbd3d60f2c35f4e5ec819191f4f3a7adbc21dec2: SomeFile conflicts with somefile
  remote: 
  remote:   Debug context:
  remote:     CaseConflict {
  remote:         changeset_id: ChangesetId(
  remote:             Blake2(273fd0b40d61b2582af82625cbd3d60f2c35f4e5ec819191f4f3a7adbc21dec2),
  remote:         ),
  remote:         path1: MPath("SomeFile"),
  remote:         path2: MPath("somefile"),
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

again, it's ok to push this to a scratch branch
  $ hgmn push -r . --to scratch/conflict2 --create
  pushing to ssh://user@dummy/repo
  searching for changes

we can't move the bookmark to a commit with a pre-existing case conflict via bookmark-only pushrebase
  $ hgmn push -r other --to main --pushvar NON_FAST_FORWARD=true
  pushing rev 6c96a55eca9d to destination ssh://user@dummy/repo bookmark main
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     While doing a bookmark-only pushrebase
  remote: 
  remote:   Root cause:
  remote:     Case conflict found in 34931495583238beb776a43e216288f3d2a73946ef3b9e72d77f83e2aafe04c9: existing/CaseConflict conflicts with existing/caseconflict
  remote: 
  remote:   Caused by:
  remote:     Failed to move bookmark
  remote:   Caused by:
  remote:     Case conflict found in 34931495583238beb776a43e216288f3d2a73946ef3b9e72d77f83e2aafe04c9: existing/CaseConflict conflicts with existing/caseconflict
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a bookmark-only pushrebase",
  remote:         source: Error {
  remote:             context: "Failed to move bookmark",
  remote:             source: CaseConflict {
  remote:                 changeset_id: ChangesetId(
  remote:                     Blake2(34931495583238beb776a43e216288f3d2a73946ef3b9e72d77f83e2aafe04c9),
  remote:                 ),
  remote:                 path1: MPath("existing/CaseConflict"),
  remote:                 path2: MPath("existing/caseconflict"),
  remote:             },
  remote:         },
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

we can't land to the other because of the pre-existing case conflict
  $ hg up -q other
  $ echo testfile > testfile
  $ hg add testfile
  $ hg commit -qm conflict3
  $ hgmn push -r . --to other
  pushing rev ecddc5d69d2c to destination ssh://user@dummy/repo bookmark other
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Case conflict found in 67c5c1587e99f6c4e6c0b0bd40411efd2f0da0559835013dcd7d1ffeb65c037e: existing/CaseConflict conflicts with existing/caseconflict
  remote: 
  remote:   Root cause:
  remote:     Case conflict found in 67c5c1587e99f6c4e6c0b0bd40411efd2f0da0559835013dcd7d1ffeb65c037e: existing/CaseConflict conflicts with existing/caseconflict
  remote: 
  remote:   Debug context:
  remote:     CaseConflict {
  remote:         changeset_id: ChangesetId(
  remote:             Blake2(67c5c1587e99f6c4e6c0b0bd40411efd2f0da0559835013dcd7d1ffeb65c037e),
  remote:         ),
  remote:         path1: MPath("existing/CaseConflict"),
  remote:         path2: MPath("existing/caseconflict"),
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

we can land something that fixes all of the case conflicts
  $ hg rm existing/CaseConflict
  $ hg amend -q
  $ hgmn push -r . --to other
  pushing rev 495ff9e58583 to destination ssh://user@dummy/repo bookmark other
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark other
