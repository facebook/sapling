#require py3exe

This test helps in keeping a track on which commands we can run on
Python 3 and see what kind of errors are coming up.
The full traceback is hidden to have a stable output.
  $ HGBIN=`which hg`

  $ for cmd in version debuginstall ; do
  >   echo $cmd
  >   $PYTHON3 $HGBIN $cmd 2>&1 2>&1 | tail -1
  > done
  version
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
  debuginstall
  no problems detected

#if test-repo
Make a clone so that any features in the developer's .hg/hgrc that
might confuse Python 3 don't break this test. When we can do commit in
Python 3, we'll stop doing this. We use e76ed1e480ef for the clone
because it has different files than 273ce12ad8f1, so we can test both
`files` from dirstate and `files` loaded from a specific revision.

  $ hg clone -r e76ed1e480ef "`dirname "$TESTDIR"`" testrepo 2>&1 | tail -1
  15 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test using -R, which exercises some URL code:
  $ $PYTHON3 $HGBIN -R testrepo files -r 273ce12ad8f1 | tail -1
  testrepo/tkmerge

Now prove `hg files` is reading the whole manifest. We have to grep
out some potential warnings that come from hgrc as yet.
  $ cd testrepo
  $ $PYTHON3 $HGBIN files -r 273ce12ad8f1
  .hgignore
  PKG-INFO
  README
  hg
  mercurial/__init__.py
  mercurial/byterange.py
  mercurial/fancyopts.py
  mercurial/hg.py
  mercurial/mdiff.py
  mercurial/revlog.py
  mercurial/transaction.py
  notes.txt
  setup.py
  tkmerge

  $ $PYTHON3 $HGBIN files -r 273ce12ad8f1 | wc -l
  \s*14 (re)
  $ $PYTHON3 $HGBIN files | wc -l
  \s*15 (re)
  $ cd ..
#endif

  $ cat > included-hgrc <<EOF
  > [extensions]
  > babar = imaginary_elephant
  > EOF
  $ cat >> $HGRCPATH <<EOF
  > %include $TESTTMP/included-hgrc
  > EOF
  $ $PYTHON3 $HGBIN version | tail -1
  *** failed to import extension babar from imaginary_elephant: *: 'imaginary_elephant' (glob)
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

Test bytes-ness of policy.policy with HGMODULEPOLICY

  $ HGMODULEPOLICY=py
  $ export HGMODULEPOLICY
  $ $PYTHON3 `which hg` debuginstall 2>&1 2>&1 | tail -1
  no problems detected
