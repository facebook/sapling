# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repo, and create test repo

  $ BLOB_TYPE="blob_files" EMIT_OBSMARKERS=1 quiet default_setup

  $ mkdir sparse
  $ cat > sparse/profile <<EOF
  > path:sparse/
  > EOF
  $ hg commit -Aqm 'initial'

  $ mkdir -p foo/foo/{foo1,foo2,foo3} bar/bar/{bar1,bar2,bar3}
  $ touch foo/foo/{foo1,foo2,foo3}/123 bar/bar/{bar1,bar2,bar3}/456
  $ hg commit -Aqm 'add files'

  $ cat >> sparse/profile <<EOF
  > # some comment
  > EOF
  $ hg commit -Aqm 'modify sparse profile'

  $ touch foo/456
  $ hg commit -Aqm 'add more files'

  $ hgmn push -q -r . --to master_bookmark --force

Setup a client repo that doesn't have any of the manifests in its local store.

  $ hgclone_treemanifest ssh://user@dummy/repo-hg test_repo --noupdate --config extensions.remotenames= -q
  $ cd test_repo
  $ hgmn pull -q -B master_bookmark

Set up some config to enable sparse profiles, get logging from fetches, and
also disable ondemand fetch to check this is overriden by sparse profiles.

  $ cat >> ".hg/hgrc" << EOF
  > [ui]
  > interactive=True
  > [remotefilelog]
  > debug=True
  > cachepath=$TESTTMP/test_repo.cache
  > [treemanifest]
  > ondemandfetch=False
  > [extensions]
  > sparse=
  > EOF

Checkout commits. Expect BFS prefetch to fill our tree

  $ hgmn up 'master_bookmark~3'
  fetching tree '' *, found via * (glob)
  2 trees fetched over * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hgmn sparse enable sparse/profile

  $ hgmn up 'master_bookmark~2'
  fetching tree '' *, based on *, found via * (glob)
  7 trees fetched over * (glob)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Now, force load the root tree for the commit we have, which simulates a hg
# cache that has the data we care about but not entire, not-changed-recently
# trees that are outside our sparse profile. We expect to see BFS
# fetching for the rest of the tree.

  $ rm -r "$TESTTMP/test_repo.cache"
  $ hgmn debuggetroottree "$(hg log -r '.' -T '{manifest}')"
  1 trees fetched over * (glob)

  $ hgmn up 'master_bookmark'
  fetching tree 'sparse' * (glob)
  1 trees fetched over * (glob)
  fetching tree '' *, based on *, found via * (glob)
  3 trees fetched over * (glob)
  fetching tree for ('bar', *) (glob)
  1 trees fetched over * (glob)
  fetching 2 trees
  2 trees fetched over * (glob)
  fetching 6 trees
  2 trees fetched over * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)

Check that we can create some commits, and that nothing breaks even if the
server does not know about our root manifest.

  $ hgmn book client

  $ cat >> sparse/profile <<EOF
  > # more comment
  > EOF
  $ hgmn commit -Aqm 'modify sparse profile again'

  $ hgmn up 'client~1'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark client)

  $ hgmn up 'client'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark client)
