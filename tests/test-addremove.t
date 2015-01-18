  $ hg init rep
  $ cd rep
  $ mkdir dir
  $ touch foo dir/bar
  $ hg -v addremove
  adding dir/bar
  adding foo
  $ hg -v commit -m "add 1"
  committing files:
  dir/bar
  foo
  committing manifest
  committing changelog
  committed changeset 0:6f7f953567a2
  $ cd dir/
  $ touch ../foo_2 bar_2
  $ hg -v addremove
  adding dir/bar_2
  adding foo_2
  $ hg -v commit -m "add 2"
  committing files:
  dir/bar_2
  foo_2
  committing manifest
  committing changelog
  committed changeset 1:e65414bf35c5
  $ cd ..
  $ hg forget foo
  $ hg -v addremove
  adding foo
  $ hg forget foo
#if windows
  $ hg -v addremove nonexistent
  nonexistent: The system cannot find the file specified
  [1]
#else
  $ hg -v addremove nonexistent
  nonexistent: No such file or directory
  [1]
#endif
  $ cd ..

  $ hg init subdir
  $ cd subdir
  $ mkdir dir
  $ cd dir
  $ touch a.py
  $ hg addremove 'glob:*.py'
  adding a.py
  $ hg forget a.py
  $ hg addremove -I 'glob:*.py'
  adding a.py
  $ hg forget a.py
  $ hg addremove
  adding dir/a.py
  $ cd ..

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
  $ cp b c
  $ hg forget b
  $ hg addremove -s 50
  adding b
  adding c

  $ rm c
#if windows
  $ hg ci -A -m "c" nonexistent
  nonexistent: The system cannot find the file specified
  abort: failed to mark all new/missing files as added/removed
  [255]
#else
  $ hg ci -A -m "c" nonexistent
  nonexistent: No such file or directory
  abort: failed to mark all new/missing files as added/removed
  [255]
#endif
  $ hg st
  ! c
  $ cd ..
