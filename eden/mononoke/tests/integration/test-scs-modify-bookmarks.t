# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Bookmark moves are asynchronous.  Function to wait for the move to happen.
  $ wait_for_bookmark_update() {
  >   local repo=$1
  >   local bookmark=$2
  >   local target=$3
  >   local attempt=1
  >   sleep 1
  >   while [[ "$(scsc lookup -R $repo -B $bookmark)" != "$target" ]]
  >   do
  >     attempt=$((attempt + 1))
  >     if [[ $attempt -gt 5 ]]
  >     then
  >        echo "bookmark move of $bookmark to $target has not happened"
  >        return 1
  >     fi
  >     sleep 1
  >   done
  > }

Setup config repo:
  $ INFINITEPUSH_ALLOW_WRITES=true \
  >   INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' \
  >   POPULATE_GIT_MAPPING=1 \
  >   setup_common_config
  $ setup_configerator_configs
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > permit_service_writes = true
  > [source_control_service.service_write_restrictions.trunk-permitted-service]
  > permitted_methods = ["move_bookmark"]
  > permitted_path_prefixes = [""]
  > permitted_bookmarks = ["trunk"]
  > [source_control_service.service_write_restrictions.new-permitted-service]
  > permitted_methods = ["create_bookmark", "move_bookmark", "delete_bookmark"]
  > permitted_path_prefixes = [""]
  > permitted_bookmark_regex = "new.*"
  > [source_control_service.service_write_restrictions.restricted-service]
  > permitted_methods = ["create_bookmark", "move_bookmark"]
  > permitted_path_prefixes = ["J", "K"]
  > permitted_bookmarks = ["other"]
  > [source_control_service.service_write_restrictions.no-paths-service]
  > permitted_methods = ["create_bookmark", "move_bookmark"]
  > permitted_bookmarks = ["nopaths"]
  > [[bookmarks]]
  > name="trunk"
  > only_fast_forward=true
  > [[bookmarks]]
  > regex=".*"
  > hooks_skip_ancestors_of=["trunk"]
  > EOF

  $ register_hook_limit_filesize_global_limit 10 'bypass_pushvar="ALLOW_LARGE_FILES=true"'

  $ cat > $TESTTMP/mononoke_tunables.json <<EOF
  > {
  >   "killswitches": {
  >     "run_hooks_on_additional_changesets": true
  >   }
  > }
  > EOF

Setup testing repo for mononoke:
  $ cd "$TESTTMP"
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server

Initial commit graph
  $ drawdag <<EOF
  > F K
  > | |
  > E J
  > |/
  > D I  # D/dlarge = file_too_large
  > | |
  > C H  # H/hlarge = file_too_large
  > | |
  > B G
  > |/
  > A
  > EOF

Trunk bookmark starts at the bottom of the graph
  $ hg book -r $A trunk

import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke

clone
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2
  $ setup_hg_client
  $ enable pushrebase remotenames infinitepush
  $ setconfig infinitepush.server=false infinitepush.branchpattern="re:scratch/.+"

start SCS server
  $ start_and_wait_for_scs_server --scuba-log-file "$TESTTMP/scuba.json"
  $ export SCSC_WRITES_ENABLED=true

move trunk forwards
  $ scsc move-bookmark -R repo --name trunk -i $B
  $ wait_for_bookmark_update repo trunk $B
  $ scsc info -R repo -B trunk
  Commit: 112478962961147124edd43549aedd1a335e44bf
  Parent: 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  Date: 1970-01-01 00:00:00 +00:00
  Author: test
  Generation: 2
  
  B

moves fail if you give the wrong existing commit
  $ scsc move-bookmark -R repo --name trunk -i $A -i $C
  error: SourceControlService::repo_move_bookmark failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Bookmark transaction failed" }
  [1]

but work if you get it right
  $ scsc move-bookmark -R repo --name trunk -i $B -i $C
  $ wait_for_bookmark_update repo trunk $C
  $ scsc info -R repo -B trunk
  Commit: 26805aba1e600a82e93661149f2313866a221a7b
  Parent: 112478962961147124edd43549aedd1a335e44bf
  Date: 1970-01-01 00:00:00 +00:00
  Author: test
  Generation: 3
  
  C

try to move trunk over a commit that fails the hooks
  $ scsc move-bookmark -R repo --name trunk -i $E
  error: SourceControlService::repo_move_bookmark failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "hooks failed:\n  limit_filesize for 88dbd25ba00277e3dfdfc642d67f2c22c75ea4f8d94f011b7f526f07b6ecc345: File size limit is 10 bytes. You tried to push file dlarge that is over the limit (14 bytes). This limit is enforced for files matching the following regex: \".*\". See https://fburl.com/landing_big_diffs for instructions." }
  [1]

use a pushvar to bypass the hook
  $ scsc move-bookmark -R repo --name trunk -i $E --pushvar ALLOW_LARGE_FILES=true
  $ wait_for_bookmark_update repo trunk $E
  $ scsc info -R repo -B trunk
  Commit: 6f95f2e47a180912e108a8dbfe9d45fc417834c3
  Parent: 1ae8aa1b3bf49310b0123e6480e709287b477346
  Date: 1970-01-01 00:00:00 +00:00
  Author: test
  Generation: 5
  
  E

creating a bookmark that already exists fails
  $ scsc create-bookmark -R repo --name trunk -i $B
  error: SourceControlService::repo_create_bookmark failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Bookmark transaction failed" }
  [1]

create a bookmark
  $ scsc create-bookmark -R repo --name newgolf -i $G
  $ wait_for_bookmark_update repo newgolf $G
  $ scsc list-bookmarks -R repo --prefix new
  newgolf                                  6fa3874a3b67598ec503160c8925af79d98522d6

create a bookmark over a commit that fails the hooks
  $ scsc create-bookmark -R repo --name newindigo -i $I
  error: SourceControlService::repo_create_bookmark failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "hooks failed:\n  limit_filesize for f896e9afb959c420bb576a31f8e001851410acbf656371a7e59477c6214b6080: File size limit is 10 bytes. You tried to push file hlarge that is over the limit (14 bytes). This limit is enforced for files matching the following regex: \".*\". See https://fburl.com/landing_big_diffs for instructions." }
  [1]

scratch bookmarks don't require hooks to pass
  $ scsc create-bookmark -R repo --name scratch/indigo -i $I

or, again, we can use a service write to make this possible
  $ scsc create-bookmark -R repo --name newindigo -i $I --service-id new-permitted-service
  $ wait_for_bookmark_update repo newindigo $I
  $ scsc list-bookmarks -R repo --prefix new
  newgolf                                  6fa3874a3b67598ec503160c8925af79d98522d6
  newindigo                                5477b0d87a7c76537e4c1da51d601564f42e94cc

creating a bookmark on a commit where the hook-failing commit is already in trunk is fine
  $ scsc create-bookmark -R repo --name newkilo -i $K
  $ wait_for_bookmark_update repo newkilo $K
  $ scsc list-bookmarks -R repo --prefix new
  newgolf                                  6fa3874a3b67598ec503160c8925af79d98522d6
  newindigo                                5477b0d87a7c76537e4c1da51d601564f42e94cc
  newkilo                                  9ff3b7f3228023fe93f2bb7f033da2541f78e725

the bookmarks have all moved
  $ cd "$TESTTMP/repo2"
  $ hgmn pull -q
  devel-warn: applied empty changegroup * (glob)
  $ hg bookmark --remote
     default/newgolf           6fa3874a3b67
     default/newindigo         5477b0d87a7c
     default/newkilo           9ff3b7f32280
     default/trunk             6f95f2e47a18

create a commit with a git SHA:
  $ hg checkout -q $F
  $ echo M > M
  $ hg add M
  $ hg commit -Am "M-git" --extra convert_revision=37b0a167e07f2b84149c918cec818ffeb183dddd --extra hg-git-rename-source=git
  $ M="$(hg log -r . -T'{node}')"
  $ hgmn push -r . --bundle-store --allow-anon
  pushing to ssh://user@dummy/repo
  searching for changes

advancing trunk over a commit with a git mapping populates the git mapping
  $ scsc lookup -R repo --git 37b0a167e07f2b84149c918cec818ffeb183dddd
  error: git sha1 '37b0a167e07f2b84149c918cec818ffeb183dddd' does not exist
  
  [1]
  $ scsc move-bookmark -R repo --name trunk -i $M
  $ scsc lookup -R repo --git 37b0a167e07f2b84149c918cec818ffeb183dddd -S bonsai,hg,git
  bonsai=cfa06ae777e72b324743e76fc764430a693fe7b903e7aa48009f89a77967ce29
  git=37b0a167e07f2b84149c918cec818ffeb183dddd
  hg=b4de048b489b9b761f8d58caddbc4b9e2d0fdf42

write restrictions prevent creating a bookmark over a path that is not allowed (relative to trunk)
  $ scsc create-bookmark -R repo --name other -i $G --service-id restricted-service
  error: SourceControlService::repo_create_bookmark failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Service \'restricted-service\' is not permitted to modify path \'G\'" }
  [1]

write restrictions don't prevent creating a bookmark that is an ancestor of trunk
  $ scsc create-bookmark -R repo --name other -i $A --service-id restricted-service

write restrictions do prevent moving a bookmark over a path that is not allowed
  $ scsc move-bookmark -R repo --name other -i $G --service-id restricted-service
  error: SourceControlService::repo_move_bookmark failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Service \'restricted-service\' is not permitted to modify path \'G\'" }
  [1]

write restrictions don't prevent moving a bookmark over a path that is permitted
this time we also use another bookmark as the target
  $ scsc move-bookmark -R repo --name other -B newkilo --service-id restricted-service

a service with no permitted paths can't create a bookmark that touches anything
  $ scsc create-bookmark -R repo --name nopaths -i $J --service-id no-paths-service
  error: SourceControlService::repo_create_bookmark failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Service \'no-paths-service\' is not permitted to modify path \'J\'" }
  [1]

trunk can't be deleted
  $ scsc delete-bookmark -R repo --name trunk
  error: SourceControlService::repo_delete_bookmark failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Deletion of \'trunk\' is prohibited" }
  [1]

nor can scratch bookmarks
  $ scsc delete-bookmark -R repo --name scratch/indigo
  error: SourceControlService::repo_delete_bookmark failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Deletion of \'scratch/indigo\' is prohibited" }
  [1]

but other bookmarks can be deleted
  $ scsc delete-bookmark -R repo --name newkilo

services can delete their permitted bookmarks
  $ scsc delete-bookmark -R repo --name newindigo -i $I --service-id new-permitted-service

deletion of a bookmark with the wrong existing commit fails
  $ scsc delete-bookmark -R repo --name newgolf -i $A
  error: SourceControlService::repo_delete_bookmark failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Bookmark transaction failed" }
  [1]

restricted services can't if they don't have the method permission
  $ scsc delete-bookmark -R repo --name other --service-id restricted-service
  error: SourceControlService::repo_delete_bookmark failed with RequestError { kind: RequestErrorKind::PERMISSION_DENIED, reason: "permission denied: service restricted-service is not permitted to call method delete_bookmark in repo" }
  [1]

  $ scsc list-bookmarks -R repo
  newgolf                                  6fa3874a3b67598ec503160c8925af79d98522d6
  other                                    9ff3b7f3228023fe93f2bb7f033da2541f78e725
  trunk                                    b4de048b489b9b761f8d58caddbc4b9e2d0fdf42

hg_sync_job can replay all these bookmark moves onto the original repo

  $ cd "$TESTTMP"
  $ mononoke_hg_sync repo-hg 1 --generate-bundles 2>&1 | grep "successful sync"
  * successful sync of entries [2] (glob)
  $ mononoke_hg_sync repo-hg 2 --generate-bundles 2>&1 | grep "successful sync"
  * successful sync of entries [3] (glob)
  $ mononoke_hg_sync_loop_regenerate repo-hg 3 2>&1 | grep "successful sync"
  * successful sync of entries [4] (glob)
  * successful sync of entries [5] (glob)
  * successful sync of entries [6] (glob)
  * successful sync of entries [7] (glob)
  * successful sync of entries [8] (glob)
  * successful sync of entries [9] (glob)
  * successful sync of entries [10] (glob)
  * successful sync of entries [11] (glob)
  * successful sync of entries [12] (glob)

  $ cd repo-hg
  $ hg bookmark
     newgolf                   6fa3874a3b67
     other                     9ff3b7f32280
     trunk                     b4de048b489b
