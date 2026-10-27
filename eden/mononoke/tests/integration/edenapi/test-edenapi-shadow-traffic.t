# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Test shadow traffic forwarding between two Mononoke (EdenAPI) server instances.
Prod server forwards requests to shadow server via the ShadowForwarderMiddleware.

  $ . "${TEST_FIXTURES}/library.sh"
  $ configure modern

Setup repo and config
  $ setup_common_config
  $ testtool_drawdag -R repo << 'EOF'
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

Start the prod server first (this sets up configerator configs via Python)
  $ start_and_wait_for_mononoke_server

Verify prod is alive
  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/health_check"
  I_AM_ALIVE (no-eol)

Clone the repo so we can use hg debugapi for POST requests
  $ hg clone -q mono:repo repo-client
  $ cd repo-client

Start the shadow server with TLS but mTLS disabled, plus --shadow-tier flag.
Disabling mTLS allows the reqwest client to connect without presenting a client cert.
  $ SHADOW_MONONOKE_ADDR_FILE="$TESTTMP/shadow_mononoke_addr.txt"
  $ rm -f "$SHADOW_MONONOKE_ADDR_FILE"
  $ if [[ "$LOCALIP" == *":"* ]]; then SHADOW_BIND_ADDR="[$LOCALIP]:0"; else SHADOW_BIND_ADDR="$LOCALIP:0"; fi
  $ render_mononoke_config
  $ GLOG_minloglevel=5 \
  >   "$MONONOKE_SERVER" \
  >   --shadow-tier \
  >   --disable-mtls \
  >   --debug \
  >   --listening-host-port "$SHADOW_BIND_ADDR" \
  >   --bound-address-file "$SHADOW_MONONOKE_ADDR_FILE" \
  >   --config-tier "$MONONOKE_CONFIG_TIER" \
  >   --tls-ca "$TEST_CERTDIR/root-ca.crt" \
  >   --tls-private-key "$TEST_CERTDIR/localhost.key" \
  >   --tls-certificate "$TEST_CERTDIR/localhost.crt" \
  >   --tls-ticket-seeds "$TEST_CERTDIR/server.pem.seeds" \
  >   --land-service-client-cert "$TEST_CERTDIR/proxy.crt" \
  >   --land-service-client-private-key "$TEST_CERTDIR/proxy.key" \
  >   --scribe-logging-directory "$TESTTMP/scribe_logs" \
  >   --no-default-scuba-dataset \
  >   --scuba-log-file "$TESTTMP/shadow_scuba.json" \
  >   --tracing-test-format \
  >   --with-dynamic-observability=true \
  >   --disable-bookmark-cache-warming \
  >   --local-configerator-path "$LOCAL_CONFIGERATOR_PATH" \
  >   --just-knobs-config-path "$MONONOKE_JUST_KNOBS_OVERRIDES_PATH" \
  >   >> "$TESTTMP/shadow_mononoke.out" 2>&1 &
  $ SHADOW_PID=$!
  $ echo "$SHADOW_PID" >> "$DAEMON_PIDS"

Wait for the shadow server to be ready
  $ export SHADOW_MONONOKE_SOCKET
  $ for i in $(seq 1 60); do
  >   if [ -r "$SHADOW_MONONOKE_ADDR_FILE" ]; then
  >     SHADOW_MONONOKE_SOCKET=$(sed 's/^.*:\([^:]*\)$/\1/' "$SHADOW_MONONOKE_ADDR_FILE")
  >     if curl -sf --insecure "https://localhost:$SHADOW_MONONOKE_SOCKET/health_check" >/dev/null 2>&1; then
  >       break
  >     fi
  >   fi
  >   sleep 1
  > done

Verify shadow is alive (HTTPS but no client cert needed due to --disable-mtls)
  $ curl -sf --insecure "https://localhost:$SHADOW_MONONOKE_SOCKET/health_check"
  I_AM_ALIVE (no-eol)

Record baseline: no EdenAPI requests processed on shadow yet
  $ jq -c 'select(.normal.log_tag == "EdenAPI Request Processed")' "$TESTTMP/shadow_scuba.json" 2>/dev/null | wc -l
  0

Helper: atomically update the shadow traffic config via temp file + mv
to avoid the ConfigStore reading a truncated file (cat > truncates before writing).
  $ write_shadow_config() {
  >   local dest="$1"
  >   local tmpf="${dest}.tmp.$$"
  >   cat > "$tmpf"
  >   mv -f "$tmpf" "$dest"
  > }

Configure shadow traffic to forward 100% of requests to the shadow server
  $ SHADOW_CONFIG="${SHADOW_TRAFFIC_CONF}/slapi"
  $ write_shadow_config "$SHADOW_CONFIG" << EOF
  > {
  >   "enabled": true,
  >   "sample_ratio": 100,
  >   "path_include": "",
  >   "path_exclude": "",
  >   "target_url": "https://localhost:$SHADOW_MONONOKE_SOCKET/edenapi",
  >   "semaphore_permits": 100,
  >   "shadow_first_timeout_ms": 5000,
  >   "shadow_first": false
  > }
  > EOF
  $ force_update_configerator

Send GET requests to prod — they should succeed and also forward to shadow
  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/edenapi/repos"
  {"repos":["repo"]} (no-eol)
  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/edenapi/repos"
  {"repos":["repo"]} (no-eol)

Give the async fire-and-forget forwarding time to complete
  $ sleep 3

Verify the shadow server received and processed forwarded GET requests
  $ jq -c 'select(.normal.log_tag == "EdenAPI Request Processed")' "$TESTTMP/shadow_scuba.json" | wc -l
  2

Test POST body forwarding: use hg debugapi to send a POST request with a body.
The listbookmarkpatterns endpoint sends a POST with CBOR-encoded body. If the body
is forwarded correctly, the shadow will process it without errors.
  $ hg debugapi -e listbookmarkpatterns -i '["master_bookmark"]'
  {"master_bookmark": "20ca2a4749a439b459125ef0f6a4f26e88ee7538"}
  $ sleep 3

Shadow should have received the POST request too (total 3)
  $ jq -c 'select(.normal.log_tag == "EdenAPI Request Processed")' "$TESTTMP/shadow_scuba.json" | wc -l
  3

Now disable shadow traffic
  $ write_shadow_config "$SHADOW_CONFIG" << EOF
  > {
  >   "enabled": false,
  >   "sample_ratio": 0,
  >   "path_include": "",
  >   "path_exclude": "",
  >   "target_url": "https://localhost:$SHADOW_MONONOKE_SOCKET/edenapi",
  >   "semaphore_permits": 100,
  >   "shadow_first_timeout_ms": 5000,
  >   "shadow_first": false
  > }
  > EOF
  $ force_update_configerator
  $ sleep 1

Send request with shadow disabled — prod should work but shadow should NOT get new requests
  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/edenapi/repos"
  {"repos":["repo"]} (no-eol)
  $ sleep 2

Shadow request count should still be 3 (no new requests forwarded)
  $ jq -c 'select(.normal.log_tag == "EdenAPI Request Processed")' "$TESTTMP/shadow_scuba.json" | wc -l
  3

Test shadow-first mode
  $ write_shadow_config "$SHADOW_CONFIG" << EOF
  > {
  >   "enabled": true,
  >   "sample_ratio": 100,
  >   "path_include": "",
  >   "path_exclude": "",
  >   "target_url": "https://localhost:$SHADOW_MONONOKE_SOCKET/edenapi",
  >   "semaphore_permits": 100,
  >   "shadow_first_timeout_ms": 5000,
  >   "shadow_first": true
  > }
  > EOF
  $ force_update_configerator
  $ sleep 1

Shadow-first: request should succeed with response body relayed from shadow
  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/edenapi/repos"
  {"repos":["repo"]} (no-eol)
  $ sleep 1

Shadow should have received one more request (total 4)
  $ jq -c 'select(.normal.log_tag == "EdenAPI Request Processed")' "$TESTTMP/shadow_scuba.json" | wc -l
  4

Test shadow-first with unreachable target — should fall back to local processing
  $ write_shadow_config "$SHADOW_CONFIG" << EOF
  > {
  >   "enabled": true,
  >   "sample_ratio": 100,
  >   "path_include": "",
  >   "path_exclude": "",
  >   "target_url": "https://localhost:1",
  >   "semaphore_permits": 100,
  >   "shadow_first_timeout_ms": 500,
  >   "shadow_first": true
  > }
  > EOF
  $ force_update_configerator
  $ sleep 1

Should still get a response via local fallback
  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/edenapi/repos"
  {"repos":["repo"]} (no-eol)
  $ sleep 2

Shadow count should still be 4 (unreachable target, no new shadow requests)
  $ jq -c 'select(.normal.log_tag == "EdenAPI Request Processed")' "$TESTTMP/shadow_scuba.json" | wc -l
  4

Kill the shadow server
  $ killandwait "$SHADOW_PID"
