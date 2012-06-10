  $ hg init rep
  $ cd rep
  $ mkdir dir
  $ touch foo dir/bar
  $ hg -v addremove
  adding dir/bar
  adding foo
  $ hg -v commit -m "add 1"
  dir/bar
  foo
  committed changeset 0:6f7f953567a2
  $ cd dir/
  $ touch ../foo_2 bar_2
  $ hg -v addremove
  adding dir/bar_2
  adding foo_2
  $ hg -v commit -m "add 2"
  dir/bar_2
  foo_2
  committed changeset 1:e65414bf35c5
  $ cd ../..

  $ hg init sim
  $ cd sim
  $ echo a > a
  $ echo a >> a
  $ echo a >> a
  $ echo c > c
  $ hg commit -Ama
  adding a
  adding c
  $ mv a b
  $ rm c
  $ echo d > d
  $ hg addremove -n -s 50 # issue 1696
  removing a
  adding b
  removing c
  adding d
  recording removal of a as rename to b (100% similar)
  $ hg addremove -s 50
  removing a
  adding b
  removing c
  adding d
  recording removal of a as rename to b (100% similar)
  $ hg commit -mb
  $ cd ..
