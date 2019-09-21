  $ setconfig extensions.treemanifest=!
revlog.parseindex must be able to parse the index file even if
an index entry is split between two 64k blocks.  The ideal test
would be to create an index file with inline data where
64k < size < 64k + 64 (64k is the size of the read buffer, 64 is
the size of an index entry) and with an index entry starting right
before the 64k block boundary, and try to read it.
We approximate that by reducing the read buffer to 1 byte.

  $ hg init a
  $ cd a
  $ echo abc > foo
  $ hg add foo
  $ hg commit -m 'add foo'
  $ echo >> foo
  $ hg commit -m 'change foo'
  $ hg log -r 0:
  changeset:   0:7c31755bf9b5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  
  changeset:   1:26333235a41c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo
  
  $ cat >> test.py << EOF
  > from edenscm.mercurial import changelog, uiconfig, vfs
  > from edenscm.mercurial.node import *
  > 
  > class singlebyteread(object):
  >     def __init__(self, real):
  >         self.real = real
  > 
  >     def read(self, size=-1):
  >         if size == 65536:
  >             size = 1
  >         return self.real.read(size)
  > 
  >     def __getattr__(self, key):
  >         return getattr(self.real, key)
  > 
  > def opener(*args):
  >     o = vfs.vfs(*args)
  >     def wrapper(*a):
  >         f = o(*a)
  >         return singlebyteread(f)
  >     return wrapper
  > 
  > cl = changelog.changelog(opener('.hg/store'), uiconfig.uiconfig())
  > print len(cl), 'revisions:'
  > for r in cl:
  >     print short(cl.node(r))
  > EOF
  $ hg debugpython -- test.py
  2 revisions:
  7c31755bf9b5
  26333235a41c

  $ cd ..

#if no-pure

Test SEGV caused by bad revision passed to reachableroots() (issue4775):

  $ cd a

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

  $ cd ..

#endif
