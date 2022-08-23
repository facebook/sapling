#chg-compatible
#debugruntest-compatible

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

  $ hg metaedit $A -v -m 'A1'
  committing changelog
  committing changelog
  committing changelog
