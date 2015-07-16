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
  > from mercurial import changelog, scmutil
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
  >     o = scmutil.opener(*args)
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

  $ cd ..

Test corrupted p1/p2 fields that could cause SEGV at parsers.c:

  $ mkdir invalidparent
  $ cd invalidparent

  $ hg clone --pull -q --config phases.publish=False ../a limit
  $ hg clone --pull -q --config phases.publish=False ../a segv
  $ rm -R limit/.hg/cache segv/.hg/cache

  $ python <<EOF
  > data = open("limit/.hg/store/00changelog.i", "rb").read()
  > for n, p in [('limit', '\0\0\0\x02'), ('segv', '\0\x01\0\0')]:
  >     # corrupt p1 at rev0 and p2 at rev1
  >     d = data[:24] + p + data[28:127 + 28] + p + data[127 + 32:]
  >     open(n + "/.hg/store/00changelog.i", "wb").write(d)
  > EOF

  $ hg debugindex -f1 limit/.hg/store/00changelog.i
     rev flag   offset   length     size   base   link     p1     p2       nodeid
       0 0000        0       63       62      0      0      2     -1 7c31755bf9b5
       1 0000       63       66       65      1      1      0      2 26333235a41c
  $ hg debugindex -f1 segv/.hg/store/00changelog.i
     rev flag   offset   length     size   base   link     p1     p2       nodeid
       0 0000        0       63       62      0      0  65536     -1 7c31755bf9b5
       1 0000       63       66       65      1      1      0  65536 26333235a41c

  $ cat <<EOF > test.py
  > import sys
  > from mercurial import changelog, scmutil
  > cl = changelog.changelog(scmutil.vfs(sys.argv[1]))
  > n0, n1 = cl.node(0), cl.node(1)
  > ops = [
  >     ('compute_phases_map_sets', lambda: cl.computephases([[0], []])),
  >     ('index_headrevs', lambda: cl.headrevs()),
  >     ('find_gca_candidates', lambda: cl.commonancestorsheads(n0, n1)),
  >     ('find_deepest', lambda: cl.ancestor(n0, n1)),
  >     ]
  > for l, f in ops:
  >     print l + ':',
  >     try:
  >         f()
  >         print 'uncaught buffer overflow?'
  >     except ValueError, inst:
  >         print inst
  > EOF

  $ python test.py limit/.hg/store
  compute_phases_map_sets: parent out of range
  index_headrevs: parent out of range
  find_gca_candidates: parent out of range
  find_deepest: parent out of range
  $ python test.py segv/.hg/store
  compute_phases_map_sets: parent out of range
  index_headrevs: parent out of range
  find_gca_candidates: parent out of range
  find_deepest: parent out of range

  $ cd ..
