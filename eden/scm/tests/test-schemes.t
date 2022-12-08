#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig experimental.allowfilepeer=True

  $ enable infinitepush remotefilelog remotenames schemes treemanifest
  $ . "$TESTDIR/library.sh"

  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }

  $ newserver server
  $ cd ..

  $ newremoterepo client1
  $ cat >> .hg/hgrc << EOF
  > [paths]
  > default = dotdot://server
  > default-push = push:server
  > normal-path = mononoke://mononoke.internal.tfbnw.net/server
  > [remotefilelog]
  > fallbackpath = fallback://server
  > [schemes]
  > dotdot = ssh://user@dummy/{1}
  > fallback = ssh://user@dummy/{1}
  > fb-test = mononoke://mononoke.internal.tfbnw.net/{1}
  > i = ssh://user@dummy/{1}
  > iw = ssh://user@dummy/{1}
  > push = ssh://user@dummy/{1}
  > z = file:\$PWD/
  > EOF
  $ setconfig infinitepush.branchpattern="re:scratch/.+"

test converting debug output for all paths

  $ hg debugexpandpaths
  paths.default=ssh://user@dummy/server (expanded from dotdot://server)
  paths.default-push=ssh://user@dummy/server (expanded from push:server)
  paths.normal-path=mononoke://mononoke.internal.tfbnw.net/server (not expanded)

check that paths are expanded

check that debugexpandscheme outputs the canonical form

  $ hg debugexpandscheme fb-test:opsfiles
  mononoke://mononoke.internal.tfbnw.net/opsfiles

check this still works if someone adds some extra slashes

  $ hg debugexpandscheme fb-test://opsfiles
  mononoke://mononoke.internal.tfbnw.net/opsfiles

expanding an unknown scheme emits the input

  $ hg debugexpandscheme foobar://this/that
  foobar://this/that

  $ mkcommit foobar
  $ hg push --create --to master
  pushing rev 582ab9cb184e to destination push:server bookmark master
  searching for changes
  exporting bookmark master
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes

  $ mkcommit something
  $ hg push -r . --to scratch/test123 --create
  pushing to push:server
  searching for changes
  remote: pushing 1 commit:
  remote:     6e16a5f9c216  something

  $ hg pull -r 6e16a5f9c216
  pulling from dotdot://server/
  no changes found
  adding changesets
  adding manifests
  adding file changes
