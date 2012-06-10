  $ hg init rep; cd rep

  $ touch empty-file
  $ python -c 'for x in range(10000): print x' > large-file

  $ hg addremove
  adding empty-file
  adding large-file

  $ hg commit -m A

  $ rm large-file empty-file
  $ python -c 'for x in range(10,10000): print x' > another-file

  $ hg addremove -s50
  adding another-file
  removing empty-file
  removing large-file
  recording removal of large-file as rename to another-file (99% similar)

  $ hg commit -m B

comparing two empty files caused ZeroDivisionError in the past

  $ hg update -C 0
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ rm empty-file
  $ touch another-empty-file
  $ hg addremove -s50
  adding another-empty-file
  removing empty-file

  $ cd ..

  $ hg init rep2; cd rep2

  $ python -c 'for x in range(10000): print x' > large-file
  $ python -c 'for x in range(50): print x' > tiny-file

  $ hg addremove
  adding large-file
  adding tiny-file

  $ hg commit -m A

  $ python -c 'for x in range(70): print x' > small-file
  $ rm tiny-file
  $ rm large-file

  $ hg addremove -s50
  removing large-file
  adding small-file
  removing tiny-file
  recording removal of tiny-file as rename to small-file (82% similar)

  $ hg commit -m B

should all fail

  $ hg addremove -s foo
  abort: similarity must be a number
  [255]
  $ hg addremove -s -1
  abort: similarity must be between 0 and 100
  [255]
  $ hg addremove -s 1e6
  abort: similarity must be between 0 and 100
  [255]

  $ cd ..

Issue1527: repeated addremove causes util.Abort

  $ hg init rep3; cd rep3
  $ mkdir d
  $ echo a > d/a
  $ hg add d/a
  $ hg commit -m 1

  $ mv d/a d/b
  $ hg addremove -s80
  removing d/a
  adding d/b
  recording removal of d/a as rename to d/b (100% similar) (glob)
  $ hg debugstate
  r   0          0 1970-01-01 00:00:00 d/a
  a   0         -1 unset               d/b
  copy: d/a -> d/b
  $ mv d/b c

no copies found here (since the target isn't in d

  $ hg addremove -s80 d
  removing d/b (glob)

copies here

  $ hg addremove -s80
  adding c
  recording removal of d/a as rename to c (100% similar) (glob)

  $ cd ..
