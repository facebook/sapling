
#require no-eden

  $ configure mutation-norecord
  $ . "$TESTDIR/library.sh"

Disable Rust strip since it does not strip manifest revlog.

  $ hginit repo
  $ cd repo
  $ enable pushrebase
  $ setconfig remotefilelog.server=True

Make some commits that include a merge.  In the merge commit, we modify a directory that is the same on both sides.
  $ drawdag << 'EOS'
  > D      # D/common/file = D
  > |\
  > B |    # B/common/file = BC
  > | C    # C/common/file = BC
  > |/
  > A      # A/common/file = A
  > EOS

Check the index for the common file.  The merge should have a single parent.
  $ hg debugindex common/file
     rev linkrev nodeid       p1           p2
       0       0 005d992c5dcf 000000000000 000000000000
       1       1 b301d594c1a4 005d992c5dcf 000000000000
       2       3 d378fb956d89 b301d594c1a4 000000000000

Check the index for the common directory.  For now we just verify the hash
matches the desired hash.
#  $ hg debugindex .hg/store/meta/common/00manifest.i
#     rev linkrev nodeid       p1           p2
#       0       0 0c8dfc956404 000000000000 000000000000
#       1       1 8b23b78bfba6 0c8dfc956404 000000000000
#       2       3 62d4c611b5da 8b23b78bfba6 000000000000

  >>> assert getenv('D').strip() == 'f84515ac47ca68143abf0f02ad053e590a048ae0'
