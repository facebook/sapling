#require mononoke

$ . "$TESTDIR/library.sh"

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
