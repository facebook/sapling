Tests for the journal extension; records bookmark locations.

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
  > testmocks=`pwd`/testmocks.py
  > EOF

Setup repo

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg commit -Aqm a
  $ echo b > a
  $ hg commit -Aqm b
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test empty journal

  $ hg journal
  previous locations of all bookmarks:
  no recorded locations
  $ hg journal foo
  previous locations of 'foo':
  no recorded locations

Test that bookmarks are tracked

  $ hg book -r tip bar
  $ hg journal bar
  previous locations of 'bar':
  1e6c11564562  book -r tip bar
  $ hg book -f bar
  $ hg journal bar
  previous locations of 'bar':
  cb9a9f314b8b  book -f bar
  1e6c11564562  book -r tip bar
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updating bookmark bar
  $ hg journal bar
  previous locations of 'bar':
  1e6c11564562  up
  cb9a9f314b8b  book -f bar
  1e6c11564562  book -r tip bar

Test that you can list all bookmarks as well as limit the list or filter on them

  $ hg book -r tip baz
  $ hg journal
  previous locations of all bookmarks:
  1e6c11564562  book -r tip baz
  1e6c11564562  up
  cb9a9f314b8b  book -f bar
  1e6c11564562  book -r tip bar
  $ hg journal --limit 2
  previous locations of all bookmarks:
  1e6c11564562  book -r tip baz
  1e6c11564562  up
  $ hg journal baz
  previous locations of 'baz':
  1e6c11564562  book -r tip baz
  $ hg journal bar
  previous locations of 'bar':
  1e6c11564562  up
  cb9a9f314b8b  book -f bar
  1e6c11564562  book -r tip bar
  $ hg journal foo
  previous locations of 'foo':
  no recorded locations

Test that verbose and commit output work

  $ hg journal --verbose
  previous locations of all bookmarks:
  000000000000 -> 1e6c11564562 foobar   1970-01-01 00:00 +0000  book -r tip baz
  cb9a9f314b8b -> 1e6c11564562 foobar   1970-01-01 00:00 +0000  up
  1e6c11564562 -> cb9a9f314b8b foobar   1970-01-01 00:00 +0000  book -f bar
  000000000000 -> 1e6c11564562 foobar   1970-01-01 00:00 +0000  book -r tip bar
  $ hg journal --commit
  previous locations of all bookmarks:
  1e6c11564562  book -r tip baz
  changeset:   1:1e6c11564562
  bookmark:    bar
  bookmark:    baz
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  1e6c11564562  up
  changeset:   1:1e6c11564562
  bookmark:    bar
  bookmark:    baz
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  cb9a9f314b8b  book -f bar
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  1e6c11564562  book -r tip bar
  changeset:   1:1e6c11564562
  bookmark:    bar
  bookmark:    baz
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  

Test for behaviour on unexpected storage version information

  $ printf '42\0' > .hg/journal
  $ hg journal
  previous locations of all bookmarks:
  abort: unknown journal file version '42'
  [255]
  $ hg book -r tip doomed
  unsupported journal file version '42'
