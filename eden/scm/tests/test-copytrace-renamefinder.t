#debugruntest-compatible

#require no-eden


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
  $ hg log -T '{node|short}\n' -r .
  b477aeec3edc

  $ hg debugcopytrace -s .^ -d . a
  {"a": "the missing file was deleted by commit b477aeec3edc in the branch rebasing onto"}
  $ hg debugcopytrace -s .^ -d . a --config copytrace.fallback-to-content-similarity=True
  {"a": "b"}

  $ hg debugcopytrace -s . -d .^ b
  {"b": "the missing file was added by commit b477aeec3edc in the branch being rebased"}
  $ hg debugcopytrace -s . -d .^ b --config copytrace.fallback-to-content-similarity=True
  {"b": "a"}
