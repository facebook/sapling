#chg-compatible

  $ . "$TESTDIR/library.sh"
  $ setconfig treemanifest.flatcompat=False

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > treemanifest=
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > EOF
  $ mkcommit root
  $ hg phase -p -r 'all()'

Clone it
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client1 -q --config extensions.treemanifest= --config treemanifest.treeonly=True
  fetching tree '' 1dd55a482f8027ebff785185b3691491312757d3
  1 trees fetched over * (glob)
  1 files fetched over * (glob) (?)
  $ cd client1
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > 
  > [treemanifest]
  > treeonly=True
  > sendtrees=True
  > EOF

  $ hg debugdrawdag <<EOS
  >   F      # A/dir1/file = A
  >  /       # A/dir2/file = A
  > C E      # A/dir3/file = A
  > |/       # A/dir4/file = A
  > B D      # B/dir1/file = B
  > |/       # B/dir2/file = B
  > A        # B/dir3/file = B
  >          # B/dir4/file = B
  >          # C/dir1/file = C
  >          # C/dir2/file = C
  >          # C/dir3/file = C
  >          # C/dir4/file = C
  >          # D/dir1/file = D
  >          # E/dir2/file = E
  >          # F/dir3/file = F
  > EOS
  $ hg bundle -f --base 'A+B+C' leaves.hg -r 'D+E+F'
  3 changesets found

The bundle should have 6 tree items in it - the root tree, and the directory tree that is modified in each of the 3 commits.

  $ hg debugbundle leaves.hg
  Stream params: {Compression: BZ}
  changegroup -- {nbchanges: 3, version: 02}
      e77dab89c4644acd044afb734c17e20046be6ae7
      97a6f48cdfe66f86ceca092b0619df4e5a99d6ec
      72e9b93c4354749519aa668d05dd8d358ec3b6c5
  b2x:treegroup2 -- {cache: False, category: manifests, version: 1}
      6 data items, 6 history items
