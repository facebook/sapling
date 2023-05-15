#debugruntest-compatible

  $ configure modernclient

Prepare repo

  $ newclientrepo repo1
  $ cat > a << EOF
  > 1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ hg ci -q -Am 'add a'

Test copytrace

  $ hg rm a
  $ cat > b << EOF
  > 1
  > 2
  > 3
  > 4
  > EOF
  $ hg ci -q -Am 'mv a -> b'

  $ hg debugcopytrace -s .^ -d . a
  {"a": null}
  $ hg debugcopytrace -s .^ -d . a --config copytrace.fallback-to-content-similarity=True
  {"a": "b"}

  $ hg debugcopytrace -s . -d .^ b
  {"b": null}
  $ hg debugcopytrace -s . -d .^ b --config copytrace.fallback-to-content-similarity=True
  {"b": "a"}
