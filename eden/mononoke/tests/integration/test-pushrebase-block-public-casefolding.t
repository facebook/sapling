# Copyright (c) Meta Platforms, Inc. and affiliates.
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

  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
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
  pushing rev ddbe318d5aca to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
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
  abort: unexpected EOL, expected netstring digit
  [255]

it's ok to push it on to a scratch bookmark, though
  $ hgmn push -r . --to scratch/conflict1 --create
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes

if we stack a commit that fixes the case conflict, we still can't land the stack
  $ hg rm caseconflict.txt
  $ hg commit -qm "fix conflict"
  $ hgmn push -r . --to main
  pushing rev cbb97717004c to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
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
  abort: unexpected EOL, expected netstring digit
  [255]

attempt to push a commit that introduces a case conflict onto main
  $ hg up -q main
  $ echo caseconflict > SomeFile
  $ hg add SomeFile
  warning: possible case-folding collision for SomeFile
  $ hg commit -qm conflict2
  $ hgmn push -r . --to main
  pushing rev 99950f688a32 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
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
  abort: unexpected EOL, expected netstring digit
  [255]

again, it's ok to push this to a scratch branch
  $ hgmn push -r . --to scratch/conflict2 --create
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes

we can't move the bookmark to a commit with a pre-existing case conflict via bookmark-only pushrebase
  $ hgmn push -r other --to main --pushvar NON_FAST_FORWARD=true
  pushing rev 2b2f2fedc926 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark main
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
  abort: unexpected EOL, expected netstring digit
  [255]

we can't land to the other if we introduce a new case conflict
  $ hg up -q other
  $ echo conflict > testfile
  $ echo conflict > TestFile
  $ hg add testfile TestFile
  warning: possible case-folding collision for testfile
  $ hg commit -qm conflict3
  $ hgmn push -r . --to other
  pushing rev 379371c4bd8a to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark other
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Case conflict found in a1b0639259bc3524b3d1db9b85b9300b1fb9f17c0c60d39e0bd64efa879c5dd5: TestFile conflicts with testfile
  remote: 
  remote:   Root cause:
  remote:     Case conflict found in a1b0639259bc3524b3d1db9b85b9300b1fb9f17c0c60d39e0bd64efa879c5dd5: TestFile conflicts with testfile
  remote: 
  remote:   Debug context:
  remote:     CaseConflict {
  remote:         changeset_id: ChangesetId(
  remote:             Blake2(a1b0639259bc3524b3d1db9b85b9300b1fb9f17c0c60d39e0bd64efa879c5dd5),
  remote:         ),
  remote:         path1: MPath("TestFile"),
  remote:         path2: MPath("testfile"),
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

we can land something that doesn't introduce a new case conflict
  $ hg hide -q .
  $ echo testfile > testfile
  $ hg add testfile
  $ hg commit -qm nonewconflict
  $ hgmn push -r . --to other
  pushing rev 951a1a92f401 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark other
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark other

we can't land if we try to make an existing case conflict worse
  $ echo conflict > existing/CASECONFLICT
  $ hg add existing/CASECONFLICT
  warning: possible case-folding collision for existing/CASECONFLICT
  $ hg commit -qm conflict4
  $ hgmn push -r . --to other
  pushing rev 13488940ae4f to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark other
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Case conflict found in b6801c5486aaa96f45805ddd8c874a5e602888e94cc2c93e44aacdc106e8ed9d: existing/CASECONFLICT conflicts with existing/CaseConflict
  remote: 
  remote:   Root cause:
  remote:     Case conflict found in b6801c5486aaa96f45805ddd8c874a5e602888e94cc2c93e44aacdc106e8ed9d: existing/CASECONFLICT conflicts with existing/CaseConflict
  remote: 
  remote:   Debug context:
  remote:     CaseConflict {
  remote:         changeset_id: ChangesetId(
  remote:             Blake2(b6801c5486aaa96f45805ddd8c874a5e602888e94cc2c93e44aacdc106e8ed9d),
  remote:         ),
  remote:         path1: MPath("existing/CASECONFLICT"),
  remote:         path2: MPath("existing/CaseConflict"),
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

we can land it if we also fix all of the related case conflicts
  $ hg rm existing/CaseConflict
  $ hg rm existing/caseconflict
  $ hg amend -q
  $ hgmn push -r . --to other
  pushing rev f53c362f9b2d to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark other
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark other
