  $ hg init debugrevlog
  $ cd debugrevlog
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ hg debugrevlog -m
  format : 1
  flags  : inline
  
  revisions     :  1
      merges    :  0 ( 0.00%)
      normal    :  1 (100.00%)
  revisions     :  1
      full      :  1 (100.00%)
      deltas    :  0 ( 0.00%)
  revision size : 44
      full      : 44 (100.00%)
      deltas    :  0 ( 0.00%)
  
  avg chain length  : 0
  compression ratio : 0
  
  uncompressed data size (min/max/avg) : 43 / 43 / 43
  full revision size (min/max/avg)     : 44 / 44 / 44
  delta size (min/max/avg)             : 0 / 0 / 0


Test internal debugstacktrace command

  $ cat > debugstacktrace.py << EOF
  > from mercurial.util import debugstacktrace, dst, sys
  > def f():
  >     dst('hello world')
  > def g():
  >     f()
  >     sys.stderr.flush()
  >     debugstacktrace(skip=-5, f=sys.stdout)
  > g()
  > EOF
  $ python debugstacktrace.py
  hello world at:
   debugstacktrace.py:8 in * (glob)
   debugstacktrace.py:5 in g
   debugstacktrace.py:3 in f
  stacktrace at:
   debugstacktrace.py:8 *in * (glob)
   debugstacktrace.py:7 *in g (glob)
   */util.py:* in debugstacktrace (glob)
