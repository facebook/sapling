#debugruntest-compatible

#require no-eden

#inprocess-hg-incompatible

  $ eagerepo
Setup

  $ enable fbcodereview
  $ setconfig extensions.arcconfig="$TESTDIR/../sapling/ext/extlib/phabricator/arcconfig.py"
  $ hg init repo
  $ cd repo

With an invalid arc configuration

  $ hg debuggraphql --query 'query TestQuery { employee { unixname } }' --variables '{}'
  {"error": "no .arcconfig found"}
  [32]

Configure arc...

  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "oauth" : "garbage_cert"}}}' > .arcconfig

And now with bad responses:

  $ cat > $TESTTMP/mockduit << EOF
  > [{"errors": [{"message": "failed, yo"}]}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debuggraphql --query 'query TestQuery { employee { unixname } }' --variables '{}'
  {"errors": [{"message": "failed, yo"}]}

Bad variable input shows an error

  $ cat > $TESTTMP/mockduit << EOF
  > [{}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debuggraphql --query 'query TestQuery { employee { unixname } }' --variables 'asdf'
  {"error": "variables input is invalid JSON"}
  [32]

Normal response is printed as JSON

  $ cat > $TESTTMP/mockduit << EOF
  > [{"data": {"employee": {"unixname": "user"}}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debuggraphql --query 'query TestQuery { employee { unixname } }' --variables '{}'
  {"data": {"employee": {"unixname": "user"}}}

Make sure we get decent error messages when .arcrc is missing credential
information.  We intentionally do not use HG_ARC_CONDUIT_MOCK for this test,
so it tries to parse the (empty) arc config files.

  $ echo '{}' > .arcrc
  $ echo '{}' > .arcconfig
  $ hg debuggraphql --query 'query TestQuery { employee { unixname } }' --variables '{}'
  {"error": "arcrc is missing user credentials. Use \"jf authenticate\" to fix, or ensure you are prepping your arcrc properly."}
  [32]

Make sure we get an error message if .arcrc is not proper JSON (for example
due to trailing commas). We do not use HG_ARC_CONDUIT_MOCK for this test,
in order for it to parse the badly formatted arc config file.

  $ echo '{,}' > ../.arcrc
  $ hg debuggraphql --query 'query TestQuery { employee { unixname } }' --variables '{}'
  {"error": "Configuration file *.arcrc is not a proper JSON file."} (glob)
  [32]
