# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export COMMIT_SCRIBE_CATEGORY=mononoke_commits
  $ export DRAFT_COMMIT_SCRIBE_CATEGORY=draft_mononoke_commits
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
  $ hgmn up -q $A
  $ echo B > B
  $ hg add B
  $ hg commit -m B
  $ COMMIT=$(hg log -r tip -T '{node}')

Make a scratch push
  $ hgmn push -r . --bundle-store --allow-anon -q
Test that nothing was recorded
  $ [[ -f "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" ]] 
  [1]

start SCS server
  $ start_and_wait_for_scs_server --scuba-log-file "$TESTTMP/scuba.json" --scribe-logging-directory "$TESTTMP/scribe_logs"
  $ export SCSC_WRITES_ENABLED=true

move trunk forwards
  $ scsc move-bookmark -R repo --name trunk -i $COMMIT
  $ wait_for_bookmark_update repo trunk $COMMIT
  $ scsc info -R repo -B trunk
  Commit: caf23a7900cb66b0324bbb183cb8088728258873
  Parent: 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  Date: 1970-01-01 00:00:00 +00:00
  Author: test
  Generation: 2
  
  B
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .bookmark
  "trunk"
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .changeset_id
  "75edd3b9ac6a39d2a7a85518578ba1e99f38e76a60ba57fb801b0b5853e65024"
