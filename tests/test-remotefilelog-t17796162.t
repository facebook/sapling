Remotefilelog appears to have issues with specific (historical?) datapack
files, see https://phabricator.intern.facebook.com/P57361795.

This test is here only to ensure we don't push out a build with this bug.

  $ echo FAIL
  Please fix remotefilelog file parsing, see t17796162
