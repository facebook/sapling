#chg-compatible
#debugruntest-compatible

  $ configure modern

  $ newext deprecatecmd <<EOF
  > from edenscm import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('testdeprecate', [], 'hg testdeprecate')
  > def testdeprecate(ui, repo, level):
  >     ui.deprecate("test-feature", "blah blah message", int(level))
  > EOF

  $ hg init client
  $ cd client

  $ hg testdeprecate 0
  devel-warn: feature 'test-feature' is deprecated: blah blah message
   at: $TESTTMP/deprecatecmd.py:6 (testdeprecate)
  $ hg blackbox | grep deprecated
  * [legacy][deprecated] blah blah message (glob)
  * [legacy][develwarn] devel-warn: feature 'test-feature' is deprecated: blah blah message (glob)

  $ hg testdeprecate 1
  warning: feature 'test-feature' is deprecated: blah blah message
  note: the feature will be completely disabled soon, so please migrate off

  $ hg testdeprecate 2
  warning: sleeping for 2 seconds because feature 'test-feature' is deprecated: blah blah message
  note: the feature will be completely disabled soon, so please migrate off

  $ hg testdeprecate 3
  abort: feature 'test-feature' is disabled: blah blah message
  (set config `deprecated.bypass-test-feature=True` to temporarily bypass this block)
  [255]

  $ hg testdeprecate 3 --config deprecated.bypass-test-feature=True
  warning: feature 'test-feature' is deprecated: blah blah message
  note: the feature will be completely disabled soon, so please migrate off

  $ hg testdeprecate 4
  abort: feature 'test-feature' is disabled: blah blah message
  [255]

  $ hg testdeprecate 4 --config deprecated.bypass-test-feature=True
  abort: feature 'test-feature' is disabled: blah blah message
  [255]
