#require test-repo jshint

  $ . "$TESTDIR/helpers-testrepo.sh"

run jshint on all tracked files ending in .js except vendored dependencies

  $ cd "`dirname "$TESTDIR"`"

  $ testrepohg locate 'set:**.js' \
  > -X mercurial/templates/static/excanvas.js \
  > 2>/dev/null \
  > | xargs jshint
