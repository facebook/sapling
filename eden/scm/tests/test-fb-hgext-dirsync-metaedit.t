#chg-compatible

  $ shorttraceback
  $ configure modern
  $ enable dirsync amend

  $ newrepo

Create an unsynced commit

  $ drawdag << 'EOS'
  > C  # C/dir1/c=c
  > |
  > B  # B/dir1/b=b
  > |
  > A  # A/dir1/a=a
  >    # drawdag.defaultfiles=false
  > EOS

Setup dirsync

  $ readconfig <<EOF
  > [dirsync]
  > sync1.1=dir1/
  > sync1.2=dir2/
  > EOF

Test metaedit
BUG: should not crash

  $ hg metaedit $A -v -m 'A1'
  committing changelog
  mirrored adding 'dir1/b' to 'dir2/b'
  committing files:
  dir1/b
  dir2/b
  committing manifest
  committing changelog
  RuntimeError: new p1 manifest (3238fc4c2915bee4faabbe8c65cc6aa918d36f70) is not the old p1 manifest (1f4be1ab4bf257b66c6fc07a9e2c91bfd3158a11)
  [255]
