#chg-compatible

  $ disable treemanifest

  $ hg init a
  $ cd a
  $ echo abc > foo
  $ hg add foo
  $ hg commit -m 'add foo'
  $ echo >> foo
  $ hg commit -m 'change foo'

Test SEGV caused by bad revision passed to reachableroots() (issue4775):

  $ hg debugpython -- <<EOF
  > from edenscm.mercurial import changelog, uiconfig, vfs
  > cl = changelog.changelog(vfs.vfs('.hg/store'), uiconfig.uiconfig())
  > print 'good heads:'
  > for head in [0, len(cl) - 1, -1]:
  >     print'%s: %r' % (head, cl.reachableroots(0, [head], [0]))
  > print 'bad heads:'
  > for head in [len(cl), 10000, -2, -10000, None]:
  >     print '%s:' % head,
  >     try:
  >         cl.reachableroots(0, [head], [0])
  >         print 'uncaught buffer overflow?'
  >     except (IndexError, TypeError) as inst:
  >         print inst
  > print 'good roots:'
  > for root in [0, len(cl) - 1, -1]:
  >     print '%s: %r' % (root, cl.reachableroots(root, [len(cl) - 1], [root]))
  > print 'out-of-range roots are ignored:'
  > for root in [len(cl), 10000, -2, -10000]:
  >     print '%s: %r' % (root, cl.reachableroots(root, [len(cl) - 1], [root]))
  > print 'bad roots:'
  > for root in [None]:
  >     print '%s:' % root,
  >     try:
  >         cl.reachableroots(root, [len(cl) - 1], [root])
  >         print 'uncaught error?'
  >     except TypeError as inst:
  >         print inst
  > EOF
  good heads:
  0: [0]
  1: [0]
  -1: []
  bad heads:
  2: head out of range
  10000: head out of range
  -2: head out of range
  -10000: head out of range
  None: an integer is required
  good roots:
  0: [0]
  1: [1]
  -1: [-1]
  out-of-range roots are ignored:
  2: []
  10000: []
  -2: []
  -10000: []
  bad roots:
  None: an integer is required
