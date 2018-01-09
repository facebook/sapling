  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastmanifest=
  > EOF

  $ hg init repo
  $ cd repo

a situation that linkrev needs to be adjusted:

  $ echo 1 > a
  $ hg commit -A a -m 1
  $ echo 2 > a
  $ hg commit -m 2
  $ hg up 0 -q
  $ echo 2 > a
  $ hg commit -m '2 again' -q

annotate calls "introrev", which calls "_adjustlinkrev". in this case,
"_adjustlinkrev" will fallback to the slow path that needs to call
manifestctx."readfast":

  $ hg annotate a
  2: 2
