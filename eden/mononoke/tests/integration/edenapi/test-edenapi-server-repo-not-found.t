# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test that the SaplingRemoteAPI (EdenAPI) server returns proper HTTP status
# codes when accessing repositories that don't exist or are not loaded on the
# current shard.
#
# - 404: Repository does not exist in the configuration
# - 503: Repository exists in the configuration but is not loaded on this shard

  $ . "${TEST_FIXTURES}/library.sh"

# Set up the default repo "repo" (REPOID=0).
  $ setup_common_config
  $ setup_configerator_configs

# Add a second repo to the config that we will NOT load on this server.
# Use a different REPOID to avoid "repoid used more than once" errors.
  $ REPOID=1 setup_mononoke_repo_config "unloaded_repo"

# Populate the primary repo so it can serve requests.
  $ quiet testtool_drawdag -R repo <<EOF
  > C
  > |
  > B
  > |
  > A
  > # bookmark: C master_bookmark
  > EOF

# Start the server with --filter-repos so only "repo" is loaded. This simulates
# a sharded deployment where unloaded_repo is configured tier-wide but is not
# assigned to this shard.
  $ start_and_wait_for_mononoke_server --filter-repos "^repo$"

# A completely unknown repo returns 404 (repository does not exist).
  $ sslcurlas client0 -s -w '\nHTTP %{http_code}\n' "https://localhost:$MONONOKE_SOCKET/edenapi/nonexistent/capabilities"
  {"message":"Repository does not exist: nonexistent","request_id":"*"} (glob)
  HTTP 404

# A repo that exists in the config but is not loaded on this shard returns a
# retriable 503 so the request can be routed to a shard that has it.
  $ sslcurlas client0 -s -w '\nHTTP %{http_code}\n' "https://localhost:$MONONOKE_SOCKET/edenapi/unloaded_repo/capabilities"
  {"message":"Repository not available on this server: unloaded_repo","request_id":"*"} (glob)
  HTTP 503

# The loaded repo still serves successfully.
  $ sslcurlas client0 -s -w '\nHTTP %{http_code}\n' "https://localhost:$MONONOKE_SOCKET/edenapi/repo/capabilities"
  ["sapling-common","commit-graph-segments","commit-cloud"]
  HTTP 200
