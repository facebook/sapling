# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

# Start up the Mononoke Git Service
  $ mononoke_git_service --shutdown-grace-period 15

  $ sslcurl -s "https://localhost:$MONONOKE_GIT_SERVICE_PORT/health_check"
  I_AM_ALIVE
  $ sslcurl -s -H 'x-fb-healthcheck-wait-time-seconds: 10' "https://localhost:$MONONOKE_GIT_SERVICE_PORT/health_check" &
  $ sleep 1

  $ termandwait $MONONOKE_GIT_SERVICE_PID
  I_AM_ALIVE
  $ tail -n10 $TESTTMP/mononoke_git_service.out | grep -E "(in flight)|(Shutting down)"
  * Still 1 requests in flight. Waiting (glob)
  * Still 1 requests in flight. Waiting (glob)
  * No requests still in flight! (glob)
  * Shutting down... (glob)
