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

Test if log-like commands work:

  $ $PYTHON3 $HGBIN tip
  changeset:   10:e76ed1e480ef
  tag:         tip
  user:        oxymoron@cinder.waste.org
  date:        Tue May 03 23:37:43 2005 -0800
  summary:     Fix linking of changeset revs when merging
  

  $ $PYTHON3 $HGBIN log -r0
  changeset:   0:9117c6561b0b
  user:        mpm@selenic.com
  date:        Tue May 03 13:16:10 2005 -0800
  summary:     Add back links from file revisions to changeset revisions
  

  $ cd ..
#endif

Test if `hg config` works:

  $ $PYTHON3 $HGBIN config
  defaults.backout=-d "0 0"
  defaults.commit=-d "0 0"
  defaults.shelve=--date "0 0"
  defaults.tag=-d "0 0"
  devel.all-warnings=true
  largefiles.usercache=$TESTTMP/.cache/largefiles
  ui.slash=True
  ui.interactive=False
  ui.mergemarkers=detailed
  ui.promptecho=True
  web.address=localhost
  web.ipv6=False

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

  $ rm included-hgrc
  $ touch included-hgrc

Test bytes-ness of policy.policy with HGMODULEPOLICY

  $ HGMODULEPOLICY=py
  $ export HGMODULEPOLICY
  $ $PYTHON3 `which hg` debuginstall 2>&1 2>&1 | tail -1
  no problems detected

`hg init` can create empty repos
`hg status works fine`

  $ $PYTHON3 `which hg` init py3repo
  $ cd py3repo
  $ echo "This is the file 'iota'." > iota
  $ $PYTHON3 $HGBIN status
  ? iota
  $ $PYTHON3 $HGBIN add iota
  $ $PYTHON3 $HGBIN status
  A iota
  $ $PYTHON3 $HGBIN commit --message 'commit performed in Python 3'
  $ $PYTHON3 $HGBIN status

TODO: bdiff is broken on Python 3 so we can't do a second commit yet,
when that works remove this rollback command.
  $ hg rollback
  repository tip rolled back to revision -1 (undo commit)
  working directory now based on revision -1

  $ mkdir A
  $ echo "This is the file 'mu'." > A/mu
  $ $PYTHON3 $HGBIN addremove
  adding A/mu
  $ $PYTHON3 $HGBIN status
  A A/mu
  A iota
  $ HGEDITOR='echo message > ' $PYTHON3 $HGBIN commit
  $ $PYTHON3 $HGBIN status

Prove the repo is valid using the Python 2 `hg`:
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 1 changesets, 2 total revisions
  $ hg log
  changeset:   0:e825505ba339
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     message
  
