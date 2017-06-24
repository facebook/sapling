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
  devel.all-warnings=true
  devel.default-date=0 0
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
`hg summary` also works!

  $ $PYTHON3 `which hg` init py3repo
  $ cd py3repo
  $ echo "This is the file 'iota'." > iota
  $ $PYTHON3 $HGBIN status
  ? iota
  $ $PYTHON3 $HGBIN add iota
  $ $PYTHON3 $HGBIN status
  A iota
  $ hg diff --nodates --git
  diff --git a/iota b/iota
  new file mode 100644
  --- /dev/null
  +++ b/iota
  @@ -0,0 +1,1 @@
  +This is the file 'iota'.
  $ $PYTHON3 $HGBIN commit --message 'commit performed in Python 3'
  $ $PYTHON3 $HGBIN status

  $ mkdir A
  $ echo "This is the file 'mu'." > A/mu
  $ $PYTHON3 $HGBIN addremove
  adding A/mu
  $ $PYTHON3 $HGBIN status
  A A/mu
  $ HGEDITOR='echo message > ' $PYTHON3 $HGBIN commit
  $ $PYTHON3 $HGBIN status
  $ $PYHON3 $HGBIN summary
  parent: 1:e1e9167203d4 tip
   message
  branch: default
  commit: (clean)
  update: (current)
  phases: 2 draft

Test weird unicode-vs-bytes stuff

  $ $PYTHON3 $HGBIN help | egrep -v '^ |^$'
  Mercurial Distributed SCM
  list of commands:
  additional help topics:
  (use 'hg help -v' to show built-in aliases and global options)

  $ $PYTHON3 $HGBIN help help | egrep -v '^ |^$'
  hg help [-ecks] [TOPIC]
  show help for a given topic or a help overview
  options ([+] can be repeated):
  (some details hidden, use --verbose to show complete help)

  $ $PYTHON3 $HGBIN help -k notopic
  abort: no matches
  (try 'hg help' for a list of topics)
  [255]

Prove the repo is valid using the Python 2 `hg`:
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 2 changesets, 2 total revisions
  $ hg log
  changeset:   1:e1e9167203d4
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     message
  
  changeset:   0:71c96e924262
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit performed in Python 3
  

  $ $PYTHON3 $HGBIN log -G
  @  changeset:   1:e1e9167203d4
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     message
  |
  o  changeset:   0:71c96e924262
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     commit performed in Python 3
  
  $ $PYTHON3 $HGBIN log -Tjson
  [
   {
    "rev": 1,
    "node": "e1e9167203d450ca2f558af628955b5f5afd4489",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [0, 0],
    "desc": "message",
    "bookmarks": [],
    "tags": ["tip"],
    "parents": ["71c96e924262969ff0d8d3d695b0f75412ccc3d8"]
   },
   {
    "rev": 0,
    "node": "71c96e924262969ff0d8d3d695b0f75412ccc3d8",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [0, 0],
    "desc": "commit performed in Python 3",
    "bookmarks": [],
    "tags": [],
    "parents": ["0000000000000000000000000000000000000000"]
   }
  ]

Show that update works now!

  $ $PYTHON3 $HGBIN up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ $PYTHON3 $HGBIN identify
  71c96e924262

branches and bookmarks also works!

  $ $PYTHON3 $HGBIN branches
  default                        1:e1e9167203d4
  $ $PYTHON3 $HGBIN bookmark book
  $ $PYTHON3 $HGBIN bookmarks
   * book                      0:71c96e924262
