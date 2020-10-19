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
  >   setup_common_config
  $ setup_configerator_configs
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > permit_service_writes = true
  > [source_control_service.service_write_restrictions.trunk-permitted-service]
  > permitted_methods = ["create_commit", "land_stack"]
  > permitted_path_prefixes = [""]
  > permitted_bookmarks = ["trunk"]
  > [source_control_service.service_write_restrictions.restricted-service]
  > permitted_methods = ["create_commit", "land_stack"]
  > permitted_path_prefixes = ["J", "K"]
  > permitted_bookmarks = ["trunk"]
  > [source_control_service.service_write_restrictions.no-paths-service]
  > permitted_methods = ["create_commit", "land_stack"]
  > permitted_bookmarks = ["trunk"]
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

Initial commit graph. 

A-C are the initial "trunk" commits.
D-E is a normal stack to land.
F-G contains a file that fails a hook
H is stacked on top of another stack, so can't be landed alone
J-K can be landed by restricted-service, but L cannot

  $ drawdag <<EOF
  > C        H
  > |        |
  > B    E   G    # G/glarge = file_too_large
  > |    |   |
  > A    D   F   I    # I/F = Fconflict
  >      |   |   |
  >      A   A   B
  > 
  > L
  > |
  > K
  > |
  > J
  > |
  > B
  > 
  > EOF

Trunk bookmark starts at C.
  $ hg book -r $C trunk

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

land stack to E
  $ scsc land-stack -R repo --name trunk -i $E -i $A
  trunk updated to: 9bc730a19041f9ec7cb33c626e811aa233efb18c
  168a83d6a475f0c2e11168f6c4e0e08c7e05961d4029c9818db2fd12d52978ca => f585351a92f85104bff7c284233c338b10eb1df7
  3468897915689598cb2cf8621f5edfcc2a954365c774a5ebf4d51eb8ed8a6a3f => 9bc730a19041f9ec7cb33c626e811aa233efb18c
  $ E_LANDED=9bc730a19041f9ec7cb33c626e811aa233efb18c
  $ wait_for_bookmark_update repo trunk $E_LANDED
  $ scsc info -R repo -B trunk
  Commit: 9bc730a19041f9ec7cb33c626e811aa233efb18c
  Parent: f585351a92f85104bff7c284233c338b10eb1df7
  Date: 1970-01-01 00:00:00 +00:00
  Author: test
  Generation: 5
  
  E

attempt to land stack to G, hooks will fail
  $ scsc land-stack -R repo --name trunk -i $G -i $A
  error: SourceControlService::repo_land_stack failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "hooks failed:\n  limit_filesize for 6b5b61e224d26706b1eb361a3900f168042bd4f7f936c64cd91963e365aec4e9: File size limit is 10 bytes. You tried to push file glarge that is over the limit (14 bytes). This limit is enforced for files matching the following regex: \".*\". See https://fburl.com/landing_big_diffs for instructions." }
  [1]

bypass the hook to land the stack
  $ scsc land-stack -R repo --name trunk -i $G -i $A --pushvar ALLOW_LARGE_FILES=true
  trunk updated to: f3be859b0ddb06d8c11c2bd43edd71528ff055a7
  1e82c5967442db2a0774744065e7f0f4ce810e9839897f81cc012959a783884b => a194cadd16930608adaa649035ad4c16930cbd0f
  6b5b61e224d26706b1eb361a3900f168042bd4f7f936c64cd91963e365aec4e9 => f3be859b0ddb06d8c11c2bd43edd71528ff055a7

can't land H now as it is based on G, which is not an ancestor of trunk
  $ scsc land-stack -R repo --name trunk -i $H -i $G
  error: SourceControlService::repo_land_stack failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Pushrebase failed: No common pushrebase root for trunk, all possible roots: {ChangesetId(Blake2(6b5b61e224d26706b1eb361a3900f168042bd4f7f936c64cd91963e365aec4e9))}" }
  [1]

try to land something that conflicts, it should fail
  $ scsc land-stack -R repo --name trunk -i $I -i $B
  error: SourceControlService::repo_land_stack failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Pushrebase failed: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath(\"F\"), right: MPath(\"F\") }]" }
  [1]

try to land something that conflicts, it should fail for services, too
  $ scsc land-stack -R repo --name trunk -i $I -i $B --service-id trunk-permitted-service
  error: SourceControlService::repo_land_stack failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Pushrebase failed: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath(\"F\"), right: MPath(\"F\") }]" }
  [1]

a service with no permitted paths can't land anything
  $ scsc land-stack -R repo --name trunk -i $J -i $B --service-id no-paths-service
  error: SourceControlService::repo_land_stack failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Service \'no-paths-service\' is not permitted to modify path \'J\'" }
  [1]

try to land J-L via the restricted service.  it's not permitted to land L
  $ scsc land-stack -R repo --name trunk -i $L -i $B --service-id restricted-service
  error: SourceControlService::repo_land_stack failed with RequestError { kind: RequestErrorKind::INVALID_REQUEST, reason: "Service \'restricted-service\' is not permitted to modify path \'L\'" }
  [1]

but it can land J-K
  $ scsc land-stack -R repo --name trunk -i $K -i $B --service-id restricted-service
  trunk updated to: 824e614da6439c13a8ba7843f58a8356ccd93062
  18ec6ac2eaecba17c293bb82bb7f1618783400bdfcde3b4c4fcef8267d4965aa => 06c3f59351756f5b933365815c447b7705f837bd
  6173a31994d38fba5f610c7ecbd603a195717364ea0444aaa6a5a0790d37634c => 824e614da6439c13a8ba7843f58a8356ccd93062
