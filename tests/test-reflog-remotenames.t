  $ $PYTHON -c 'import remotenames' || exit 80
  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/reflog.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > reflog=$TESTTMP/reflog.py
  > remotenames=
  > [remotenames]
  > rename.default=remote
  > EOF

  $ hg init remote
  $ cd remote
  $ touch a
  $ hg commit -A -m 'a commit' -q
  devel-warn: use dirstate.write with repo.currenttransaction() at: * (recorddirstateparents) (glob)
  devel-warn: use dirstate.write with repo.currenttransaction() at: * (recorddirstateparents) (glob)
  $ hg book bmwillnotmove
  $ hg book bm

Test reflog with remote bookmarks works on clone
  $ cd ..
  $ hg clone remote local -q
  devel-warn: use dirstate.write with repo.currenttransaction() at: * (recorddirstateparents) (glob)
  $ cd local
  $ hg reflog remote/bm
  Previous locations of 'remote/bm':
  94cf1ae9e2c8  clone remote local -q

Test reflog with remote bookmarks works on pull
  $ cd ../remote
  $ hg up bm -q
  $ echo 'modified' > a
  $ hg commit -m 'a second commit' -q
  devel-warn: use dirstate.write with repo.currenttransaction() at: * (recorddirstateparents) (glob)
  $ cd ../local
  $ hg pull -q
  $ hg reflog remote/bm
  Previous locations of 'remote/bm':
  b720e98e7160  pull -q
  94cf1ae9e2c8  clone remote local -q

Test reflog with remote bookmarks works after push
  $ hg up remote/bm -q
  devel-warn: use dirstate.write with repo.currenttransaction() at: * (recorddirstateparents) (glob)
  devel-warn: use dirstate.write with repo.currenttransaction() at: * (recorddirstateparents) (glob)
  $ echo 'modified locally' > a
  $ hg commit -m 'local commit' -q
  devel-warn: use dirstate.write with repo.currenttransaction() at: * (recorddirstateparents) (glob)
  $ hg push --to bm -q
  $ hg reflog remote/bm
  Previous locations of 'remote/bm':
  869ef7e9b417  push --to bm -q
  b720e98e7160  pull -q
  94cf1ae9e2c8  clone remote local -q

Test second remotebookmark has not been clobbered or has moved since clone
  $ hg reflog remote/bmwillnotmove
  Previous locations of 'remote/bmwillnotmove':
  94cf1ae9e2c8  clone remote local -q

