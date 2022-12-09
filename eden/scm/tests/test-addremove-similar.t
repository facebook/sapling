#chg-compatible
#debugruntest-compatible

  $ hg init rep; cd rep

  $ touch empty-file
  $ hg debugsh -c 'for x in range(10000): ui.write("%s\n" % x)' > large-file

  $ hg addremove
  adding empty-file
  adding large-file

  $ hg commit -m A

  $ rm large-file empty-file
  $ hg debugsh -c 'for x in range(10,10000): ui.write("%s\n" % x)' > another-file

  $ hg addremove -s50
  adding another-file
  removing empty-file
  removing large-file
  recording removal of large-file as rename to another-file (99% similar)

  $ hg commit -m B

comparing two empty files caused ZeroDivisionError in the past

  $ hg goto -C 'desc(A)'
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ rm empty-file
  $ touch another-empty-file
  $ hg addremove -s50
  adding another-empty-file
  removing empty-file

  $ cd ..

  $ hg init rep2; cd rep2

  $ hg debugsh -c 'for x in range(10000): ui.write("%s\n" % x)' > large-file
  $ hg debugsh -c 'for x in range(50): ui.write("%s\n" % x)' > tiny-file

  $ hg addremove
  adding large-file
  adding tiny-file

  $ hg commit -m A

  $ hg debugsh -c 'for x in range(70): ui.write("%s\n" % x)' > small-file
  $ rm tiny-file
  $ rm large-file

  $ hg addremove -s50
  removing large-file
  adding small-file
  removing tiny-file
  recording removal of tiny-file as rename to small-file (82% similar)

  $ hg commit -m B

should be sorted by path for stable result

  $ for i in `seq 0 9`; do
  >     cp small-file $i
  > done
  $ rm small-file
  $ hg addremove
  adding 0
  adding 1
  adding 2
  adding 3
  adding 4
  adding 5
  adding 6
  adding 7
  adding 8
  adding 9
  removing small-file
  recording removal of small-file as rename to 0 (100% similar)
  recording removal of small-file as rename to 1 (100% similar)
  recording removal of small-file as rename to 2 (100% similar)
  recording removal of small-file as rename to 3 (100% similar)
  recording removal of small-file as rename to 4 (100% similar)
  recording removal of small-file as rename to 5 (100% similar)
  recording removal of small-file as rename to 6 (100% similar)
  recording removal of small-file as rename to 7 (100% similar)
  recording removal of small-file as rename to 8 (100% similar)
  recording removal of small-file as rename to 9 (100% similar)
  $ hg commit -m '10 same files'

pick one from many identical files

  $ cp 0 a
  $ rm `seq 0 9`
  $ hg addremove
  removing 0
  removing 1
  removing 2
  removing 3
  removing 4
  removing 5
  removing 6
  removing 7
  removing 8
  removing 9
  adding a
  recording removal of 0 as rename to a (100% similar)
  $ hg revert -aq

pick one from many similar files

  $ cp 0 a
  $ for i in `seq 0 9`; do
  >     echo $i >> $i
  > done
  $ hg commit -m 'make them slightly different'
  $ rm `seq 0 9`
  $ hg addremove -s50
  removing 0
  removing 1
  removing 2
  removing 3
  removing 4
  removing 5
  removing 6
  removing 7
  removing 8
  removing 9
  adding a
  recording removal of 0 as rename to a (99% similar)
  $ hg commit -m 'always the same file should be selected'

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

Issue1527: repeated addremove causes Abort

  $ hg init rep3; cd rep3
  $ mkdir d
  $ echo a > d/a
  $ hg add d/a
  $ hg commit -m 1

  $ mv d/a d/b
  $ hg addremove -s80
  removing d/a
  adding d/b
  recording removal of d/a as rename to d/b (100% similar)
  $ hg debugstate
  r   0          0 unset               d/a
  a   0         -1 unset               d/b
  copy: d/a -> d/b
  $ mv d/b c

no copies found here (since the target isn't in d

  $ hg addremove -s80 d
  removing d/b

copies here

  $ hg addremove -s80
  adding c
  recording removal of d/a as rename to c (100% similar)

  $ cd ..
