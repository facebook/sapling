#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > EOF
  $ mkcommit root
  $ hg debugmakepublic -r 'all()'

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
      9aa450467c3cff6ce906129a8d87e2414c8a3adb 
      9ccae32052b23cce0b4771389f588b8d98787a5f 
      b3ec0b09ac898d08a4b9ebe89ab54f30ef2eca99 
      b5a05191267df8533dd03dfb0cf897eb862c702e dir1
      9dc6720799554340d343db656f2181f9c99590f0 dir2
      c355490c72c78063d7e05398f70f4897f407ce07 dir3
