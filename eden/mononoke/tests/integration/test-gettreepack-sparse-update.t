# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ enable remotenames

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
  fetching tree for ('', 4ccb43944747fdc11a890fcae40e0bc0ac6732da)
  fetching tree '' 4ccb43944747fdc11a890fcae40e0bc0ac6732da
  1 trees fetched over 0.00s
  fetching tree for ('sparse', 24c75048c8e4debd244f3d2a15ff6442906f6702)
  fetching tree 'sparse' 24c75048c8e4debd244f3d2a15ff6442906f6702
  1 trees fetched over 0.00s
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hgmn sparse enable sparse/profile

  $ hgmn up 'master_bookmark~2'
  fetching tree for ('', e6226c902ed8e9cd5583dcae4de931e10a4e267a)
  fetching tree '' e6226c902ed8e9cd5583dcae4de931e10a4e267a
  1 trees fetched over 0.00s
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Now, force load the root tree for the commit we have, which simulates a hg
# cache that has the data we care about but not entire, not-changed-recently
# trees that are outside our sparse profile. We expect to see BFS
# fetching for the rest of the tree.

  $ rm -r "$TESTTMP/test_repo.cache"
  $ hgmn debuggetroottree "$(hg log -r '.' -T '{manifest}')"
  fetching tree for ('', e6226c902ed8e9cd5583dcae4de931e10a4e267a)
  fetching tree '' e6226c902ed8e9cd5583dcae4de931e10a4e267a
  1 trees fetched over * (glob)

  $ hgmn up 'master_bookmark' --config sparse.force_full_prefetch_on_sparse_profile_change=True
  fetching tree for ('sparse', 24c75048c8e4debd244f3d2a15ff6442906f6702)
  fetching tree 'sparse' * (glob)
  1 trees fetched over * (glob)
  fetching tree for ('', 625a148fb84b273821a78aaa49a31e83da4a7164)
  fetching tree '' 625a148fb84b273821a78aaa49a31e83da4a7164
  1 trees fetched over * (glob)
  fetching tree for ('sparse', e738d530b4579275fc0b50efbe7204cb7b4d8266)
  fetching tree 'sparse' e738d530b4579275fc0b50efbe7204cb7b4d8266
  1 trees fetched over 0.00s
  fetching 2 trees
  2 trees fetched over * (glob)
  fetching 2 trees
  2 trees fetched over * (glob)
  fetching 6 trees
  2 trees fetched over * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  2 files fetched over 2 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)

# Now, force load the root tree for the commit again, and do update to master_bookmark
# without force_full_prefetch_on_sparse_profile_change set. Note that we fetch less trees

  $ hgmn up 'master_bookmark~2' -q
  $ rm -r "$TESTTMP/test_repo.cache"
  $ hgmn debuggetroottree "$(hg log -r '.' -T '{manifest}')"
  fetching tree for ('', e6226c902ed8e9cd5583dcae4de931e10a4e267a)
  fetching tree '' e6226c902ed8e9cd5583dcae4de931e10a4e267a
  1 trees fetched over * (glob)
  $ hgmn up 'master_bookmark'
  fetching tree for ('sparse', 24c75048c8e4debd244f3d2a15ff6442906f6702)
  fetching tree 'sparse' * (glob)
  1 trees fetched over * (glob)
  fetching tree for ('', 625a148fb84b273821a78aaa49a31e83da4a7164)
  fetching tree '' 625a148fb84b273821a78aaa49a31e83da4a7164
  1 trees fetched over 0.00s
  fetching tree for ('sparse', e738d530b4579275fc0b50efbe7204cb7b4d8266)
  fetching tree 'sparse' e738d530b4579275fc0b50efbe7204cb7b4d8266
  1 trees fetched over 0.00s
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

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
