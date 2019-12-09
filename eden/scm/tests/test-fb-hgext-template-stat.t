#chg-compatible

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > stat=
  > EOF

Setup repo

  $ hg init repo
  $ cd repo

Test template stat

  $ hg log -r . -T '{stat()}'

  $ $PYTHON $TESTDIR/seq.py 50 > a
  $ hg add a
  $ $PYTHON $TESTDIR/seq.py 26 75 > b
  $ hg add b
  $ hg commit -m "Added a and b with 50 lines each"
  $ hg log -r . -T '{stat()}'
   a |  50 ++++++++++++++++++++++++++++++++++++++++++++++++++
   b |  50 ++++++++++++++++++++++++++++++++++++++++++++++++++
   2 files changed, 100 insertions(+), 0 deletions(-)

  $ COLUMNS=20 hg log -r . -T '{stat()}'
   a |  50 +++++++++++
   b |  50 +++++++++++
   2 files changed, 100 insertions(+), 0 deletions(-)

  $ $PYTHON $TESTDIR/seq.py 50 > b
  $ $PYTHON $TESTDIR/seq.py 26 75 > a
  $ hg commit -m "Swapped the files"
  $ hg log -r . -T '{stat()}'
   a |  50 +++++++++++++++++++++++++-------------------------
   b |  50 +++++++++++++++++++++++++-------------------------
   2 files changed, 50 insertions(+), 50 deletions(-)

  $ COLUMNS=20 hg log -r . -T '{stat()}'
   a |  50 +++++-----
   b |  50 +++++-----
   2 files changed, 50 insertions(+), 50 deletions(-)

  $ mkdir dir
  $ $PYTHON $TESTDIR/seq.py 50 > dir/a
  $ hg add dir/a
  $ hg commit -m "Added file with 50 lines inside directory dir"
  $ hg log -r . -T '{stat()}'
   dir/a |  50 ++++++++++++++++++++++++++++++++++++++++++++++++++
   1 files changed, 50 insertions(+), 0 deletions(-)

  $ COLUMNS=20 hg log -r . -T '{stat()}'
   dir/a |  50 ++++++++++
   1 files changed, 50 insertions(+), 0 deletions(-)

  $ $PYTHON $TESTDIR/seq.py 41 60 > dir/a
  $ hg commit -m "Modified file inside directory dir"
  $ hg log -r . -T '{stat()}'
   dir/a |  50 ++++++++++----------------------------------------
   1 files changed, 10 insertions(+), 40 deletions(-)

  $ COLUMNS=20 hg log -r . -T '{stat()}'
   dir/a |  50 ++--------
   1 files changed, 10 insertions(+), 40 deletions(-)

  $ $PYTHON << EOF
  > with open('binary', 'wb') as f:
  >     f.write(b'\x00\x01\x02\x00' * 10)
  > EOF
  $ hg add binary
  $ hg commit -m "Added binary file"
  $ hg log -r . -T '{stat()}'
   binary |  Bin 
   1 files changed, 0 insertions(+), 0 deletions(-)
  $ hg log -r . -T '{stat("status")}'
  added   binary |  Bin 
   1 files changed, 0 insertions(+), 0 deletions(-)

  $ hg rm -q binary a
  $ echo 3 >> b
  $ echo 4 >> c
  $ hg add c
  $ hg log -r 'wdir()' -T '{stat(status)}'
  removed a      |   50 --------------------------------------------------
  changed b      |    1 +
  removed binary |  Bin 
  added   c      |    1 +
   4 files changed, 2 insertions(+), 50 deletions(-)
