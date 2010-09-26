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
  > from mercurial import changelog, util
  > from mercurial.node import *
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
  >     o = util.opener(*args)
  >     def wrapper(*a):
  >         f = o(*a)
  >         return singlebyteread(f)
  >     return wrapper
  > 
  > cl = changelog.changelog(opener('.hg/store'))
  > print len(cl), 'revisions:'
  > for r in cl:
  >     print short(cl.node(r))
  > EOF
  $ python test.py
  2 revisions:
  7c31755bf9b5
  26333235a41c
