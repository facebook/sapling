  $ newrepo
  $ setconfig ui.allowemptycommit=1
  $ enable amend

  $ HGEDITOR=true hg commit -m 1 -e
  $ HGEDITOR=true hg commit --amend -m 2 -e
  $ HGEDITOR='echo 3 >' hg metaedit

All 3 files are here:

  $ python << EOF
  > import os
  > names = os.listdir('.hg/edit-tmp')
  > print(names)
  > for name in names:
  >     os.utime(os.path.join('.hg/edit-tmp', name), (0, 0)) 
  > EOF
  ['*', '*', '*'] (glob)

  $ HGEDITOR=true hg commit -m 4 -e

Those files will be cleaned up since they have ancient mtime:

  $ python << EOF
  > import os
  > print(os.listdir('.hg/edit-tmp'))
  > EOF
  ['*'] (glob)

Verify that a folder in .hg/edit-tmp doesn't crash hg:

  $ mkdir .hg/edit-tmp/foo
  $ HGEDITOR=true hg commit -m 5 -e
