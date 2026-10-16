
#require no-eden

#testcases tree-store tree-manifest

#if tree-manifest
  $ setconfig experimental.pathhistory-treemanifest=true
#endif

Prepare repo

  $ newclientrepo repo1
  $ cat > a << EOF
  > 1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ sl ci -q -Am 'add a'

Test copytrace

  $ sl rm a
  $ cat > b << EOF
  > 1
  > 2
  > 3
  > 4
  > EOF
  $ sl ci -q -Am 'mv a -> b'
  $ sl log -T '{node|short}\n' -r .
  b477aeec3edc

  $ sl debugcopytrace -s .^ -d . a
  {"a": "the missing file was deleted by commit b477aeec3edc in the branch rebasing onto"}
  $ sl debugcopytrace -s .^ -d . a --config copytrace.fallback-to-content-similarity=True
  {"a": "b"}

  $ sl debugcopytrace -s . -d .^ b
  {"b": "the missing file was added by commit b477aeec3edc in the branch being rebased"}
  $ sl debugcopytrace -s . -d .^ b --config copytrace.fallback-to-content-similarity=True
  {"b": "a"}
