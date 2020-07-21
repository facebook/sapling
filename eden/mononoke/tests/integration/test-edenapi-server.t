# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Start up EdenAPI server.
  $ setup_mononoke_config
  $ setup_configerator_configs
  $ start_edenapi_server

Hit health check endpoint.
  $ sslcurl -s "$EDENAPI_URI/health_check"
  I_AM_ALIVE (no-eol)

List repos.
  $ sslcurl -s "$EDENAPI_URI/repos"
  {"repos":["repo"]} (no-eol)
