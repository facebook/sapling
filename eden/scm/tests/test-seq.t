  $ $TESTDIR/seq.py 1 10 > file
  $ cat file
  1
  2
  3
  4
  5
  6
  7
  8
  9
  10
  $ cat file | f --hexdump
  
  0000: 31 0a 32 0a 33 0a 34 0a 35 0a 36 0a 37 0a 38 0a |1.2.3.4.5.6.7.8.|
  0010: 39 0a 31 30 0a                                  |9.10.|
  $ $TESTDIR/seq.py 0 0
  0
  $ $TESTDIR/seq.py 1
  1
