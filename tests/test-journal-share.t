Journal extension test: tests the share extension support

  $ cat >> testmocks.py << EOF
  > # mock out util.getuser() and util.makedate() to supply testable values
  > import os
  > from mercurial import util
  > def mockgetuser():
  >     return 'foobar'
  > 
  > def mockmakedate():
  >     filename = os.path.join(os.environ['TESTTMP'], 'testtime')
  >     try:
  >         with open(filename, 'rb') as timef:
  >             time = float(timef.read()) + 1
  >     except IOError:
  >         time = 0.0
  >     with open(filename, 'wb') as timef:
  >         timef.write(str(time))
  >     return (time, 0)
  > 
  > util.getuser = mockgetuser
  > util.makedate = mockmakedate
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > journal=
  > share=
  > testmocks=`pwd`/testmocks.py
  > [remotenames]
  > rename.default=remote
  > EOF

  $ hg init repo
  $ cd repo
  $ hg bookmark bm
  $ touch file0
  $ hg commit -Am file0-added
  adding file0
  $ hg journal --all
  previous locations of the working copy and bookmarks:
  0fd3805711f9  .         commit -Am file0-added
  0fd3805711f9  bm        commit -Am file0-added

A shared working copy initially receives the same bookmarks and working copy

  $ cd ..
  $ hg share repo shared1
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd shared1
  $ hg journal --all
  previous locations of the working copy and bookmarks:
  0fd3805711f9  .         share repo shared1

unless you explicitly share bookmarks

  $ cd ..
  $ hg share --bookmarks repo shared2
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd shared2
  $ hg journal --all
  previous locations of the working copy and bookmarks:
  0fd3805711f9  .         share --bookmarks repo shared2
  0fd3805711f9  bm        commit -Am file0-added

Moving the bookmark in the original repository is only shown in the repository
that shares bookmarks

  $ cd ../repo
  $ touch file1
  $ hg commit -Am file1-added
  adding file1
  $ cd ../shared1
  $ hg journal --all
  previous locations of the working copy and bookmarks:
  0fd3805711f9  .         share repo shared1
  $ cd ../shared2
  $ hg journal --all
  previous locations of the working copy and bookmarks:
  4f354088b094  bm        commit -Am file1-added
  0fd3805711f9  .         share --bookmarks repo shared2
  0fd3805711f9  bm        commit -Am file0-added

But working copy changes are always 'local'

  $ cd ../repo
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark bm)
  $ hg journal --all
  previous locations of the working copy and bookmarks:
  0fd3805711f9  .         up 0
  4f354088b094  .         commit -Am file1-added
  4f354088b094  bm        commit -Am file1-added
  0fd3805711f9  .         commit -Am file0-added
  0fd3805711f9  bm        commit -Am file0-added
  $ cd ../shared2
  $ hg journal --all
  previous locations of the working copy and bookmarks:
  4f354088b094  bm        commit -Am file1-added
  0fd3805711f9  .         share --bookmarks repo shared2
  0fd3805711f9  bm        commit -Am file0-added
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg journal
  previous locations of '.':
  0fd3805711f9  up 0
  4f354088b094  up tip
  0fd3805711f9  share --bookmarks repo shared2

Unsharing works as expected; the journal remains consistent

  $ cd ../shared1
  $ hg unshare
  $ hg journal --all
  previous locations of the working copy and bookmarks:
  0fd3805711f9  .         share repo shared1
  $ cd ../shared2
  $ hg unshare
  $ hg journal --all
  previous locations of the working copy and bookmarks:
  0fd3805711f9  .         up 0
  4f354088b094  .         up tip
  4f354088b094  bm        commit -Am file1-added
  0fd3805711f9  .         share --bookmarks repo shared2
  0fd3805711f9  bm        commit -Am file0-added

New journal entries in the source repo no longer show up in the other working copies

  $ cd ../repo
  $ hg bookmark newbm -r tip
  $ hg journal newbm
  previous locations of 'newbm':
  4f354088b094  bookmark newbm -r tip
  $ cd ../shared2
  $ hg journal newbm
  previous locations of 'newbm':
  no recorded locations

This applies for both directions

  $ hg bookmark shared2bm -r tip
  $ hg journal shared2bm
  previous locations of 'shared2bm':
  4f354088b094  bookmark shared2bm -r tip
  $ cd ../repo
  $ hg journal shared2bm
  previous locations of 'shared2bm':
  no recorded locations
