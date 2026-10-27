# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Test shadow traffic forwarding between two Mononoke Git server instances.
Prod server forwards requests to shadow server via the ShadowForwarderMiddleware.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

Start the prod git server first (this sets up configerator configs)
  $ mononoke_git_service

Verify prod is alive
  $ sslcurl -s "https://localhost:$MONONOKE_GIT_SERVICE_PORT/health_check"
  I_AM_ALIVE

Start the shadow git server on plain HTTP (no TLS) with --shadow-tier flag
  $ SHADOW_GIT_ADDR_FILE="$TESTTMP/shadow_git_service_addr.txt"
  $ SHADOW_GIT_LOG="$TESTTMP/shadow_git_service.out"
  $ rm -f "$SHADOW_GIT_ADDR_FILE"
  $ render_mononoke_config
  $ GLOG_minloglevel=5 \
  >   "$MONONOKE_GIT_SERVER" \
  >   --shadow-tier \
  >   --listen-host "$LOCALIP" \
  >   --listen-port 0 \
  >   --scuba-log-file "$TESTTMP/shadow_git_scuba.json" \
  >   --log-level DEBUG \
  >   --config-tier "$MONONOKE_CONFIG_TIER" \
  >   --bound-address-file "$SHADOW_GIT_ADDR_FILE" \
  >   --tracing-test-format \
  >   --local-configerator-path "$LOCAL_CONFIGERATOR_PATH" \
  >   --just-knobs-config-path "$MONONOKE_JUST_KNOBS_OVERRIDES_PATH" \
  >   "${CACHE_ARGS[@]}" >> "$SHADOW_GIT_LOG" 2>&1 &
  $ SHADOW_GIT_PID=$!
  $ echo "$SHADOW_GIT_PID" >> "$DAEMON_PIDS"

Wait for shadow git server
  $ export SHADOW_GIT_PORT
  $ for i in $(seq 1 60); do
  >   if [ -r "$SHADOW_GIT_ADDR_FILE" ]; then
  >     SHADOW_GIT_PORT=$(sed 's/^.*:\([^:]*\)$/\1/' "$SHADOW_GIT_ADDR_FILE")
  >     if curl -sf "http://localhost:$SHADOW_GIT_PORT/health_check" >/dev/null 2>&1; then
  >       break
  >     fi
  >   fi
  >   sleep 1
  > done

Verify shadow is alive (plain HTTP)
  $ curl -sf "http://localhost:$SHADOW_GIT_PORT/health_check"
  I_AM_ALIVE

Record baseline: no git requests processed on shadow yet
  $ jq -c 'select(.normal.log_tag == "MononokeGit Request Processed" and .normal.http_path != "/health_check")' "$TESTTMP/shadow_git_scuba.json" 2>/dev/null | wc -l
  0

Configure shadow traffic to forward 100% to shadow git server (plain HTTP)
  $ SHADOW_GIT_CONFIG="${SHADOW_TRAFFIC_CONF}/git"
  $ cat > "$SHADOW_GIT_CONFIG" << EOF
  > {
  >   "enabled": true,
  >   "sample_ratio": 100,
  >   "path_include": "",
  >   "path_exclude": "",
  >   "target_url": "http://localhost:$SHADOW_GIT_PORT/repos/git/ro",
  >   "semaphore_permits": 100,
  >   "shadow_first_timeout_ms": 5000,
  >   "shadow_first": false
  > }
  > EOF
Wait for the file-based ConfigStore to poll and pick up the new config.
The git server does not have a force_update_configerator control endpoint
like the SLAPI server, so we rely on the 1-second file poll interval.
  $ sleep 2

Clone from prod server — should succeed and forward to shadow
  $ cd "$TESTTMP"
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...
  remote: Client correlator: * (glob)
  remote: Converting HAVE Git commits to Bonsais* (glob)
  remote: Converting WANT Git commits to Bonsais* (glob)
  remote: Collecting Bonsai commits to send to client* (glob)
  remote: Counting number of objects to be sent in packfile* (glob)
  remote: Generating trees and blobs stream* (glob)
  remote: Generating commits stream* (glob)
  remote: Generating tags stream* (glob)
  remote: Sending packfile stream* (glob)

Give the async forwarding time to complete
  $ sleep 3

Verify cloned content is correct
  $ cd "$REPONAME"
  $ cat file1
  this is file1
  $ cat file2
  this is file2
  $ cd "$TESTTMP"

Verify the shadow git server received forwarded requests
  $ jq -c 'select(.normal.log_tag == "MononokeGit Request Processed" and .normal.http_path != "/health_check")' "$TESTTMP/shadow_git_scuba.json" | wc -l
  3

Now disable shadow traffic
  $ cat > "$SHADOW_GIT_CONFIG" << EOF
  > {
  >   "enabled": false,
  >   "sample_ratio": 0,
  >   "path_include": "",
  >   "path_exclude": "",
  >   "target_url": "http://localhost:$SHADOW_GIT_PORT/repos/git/ro",
  >   "semaphore_permits": 100,
  >   "shadow_first_timeout_ms": 5000,
  >   "shadow_first": false
  > }
  > EOF
  $ sleep 1

Requests should still work on prod with shadow disabled
  $ sslcurl -s "https://localhost:$MONONOKE_GIT_SERVICE_PORT/health_check"
  I_AM_ALIVE

Test shadow-first with unreachable target — should fall back to local
  $ cat > "$SHADOW_GIT_CONFIG" << EOF
  > {
  >   "enabled": true,
  >   "sample_ratio": 100,
  >   "path_include": "",
  >   "path_exclude": "",
  >   "target_url": "http://localhost:1",
  >   "semaphore_permits": 100,
  >   "shadow_first_timeout_ms": 500,
  >   "shadow_first": true
  > }
  > EOF
  $ sleep 1

Should still work via local fallback
  $ sslcurl -s "https://localhost:$MONONOKE_GIT_SERVICE_PORT/health_check"
  I_AM_ALIVE

Kill the shadow server
  $ killandwait "$SHADOW_GIT_PID"
