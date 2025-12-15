  $ enable smartlog

Rejects invalid size-factors:

  $ sl init --config format.use-virtual-repo-with-size-factor=-1 virtual-1
  abort: format.use-virtual-repo-with-size-factor must be between 0 and 34
  [255]
  $ sl init --config format.use-virtual-repo-with-size-factor=35 virtual35
  abort: format.use-virtual-repo-with-size-factor must be between 0 and 34
  [255]

Virtual repo with size-factor=2:

  $ sl init --config format.use-virtual-repo-with-size-factor=2 virtual2
  $ cd virtual2

Smartlog works:

  $ sl
  o  commit:      c20cdc010000
  │  bookmark:    virtual/main
  ~  user:        test <test@example.com>
     date:        Sat Oct 25 09:35:59 2025 +0000
     summary:     synthetic commit 121869

Total file count and size is reasonable (~80MB):

  $ sl go 'roots(all())' -q
  $ FILES=$TESTTMP/files
  $ sl files > $FILES

  >>> import os, stat
  >>> def check_count_file_size():
  ...     """Check $FILES is sane (no dups, no dirs). Return count and size."""
  ...     with open(getenv('FILES')) as f:
  ...         paths = [p.strip() for p in f]
  ...     assert len(paths) == len(set(paths)), 'paths should be unique'
  ...     total_size = 0
  ...     for path in paths:
  ...         st = os.lstat(path)
  ...         assert not stat.S_ISDIR(st.st_mode), f'{path} should not be a dir'
  ...         # skip non-regular files (e.g. symlinks) that have different sizes on Windows
  ...         if stat.S_ISREG(st.st_mode):
  ...             total_size += st.st_size
  ...     return len(paths), total_size

  >>> check_count_file_size()
  (6084, 84032419)

Checkout virtual/main with more files (~800MB):

  $ sl go 'virtual/main' -q
  $ sl files > $FILES

  >>> check_count_file_size()
  (57364, 769126770)

Virtual repo with size-factor=0 works too:

  $ cd
  $ sl init --config format.use-virtual-repo-with-size-factor=0 virtual0
  $ cd virtual0
  $ sl status --change 'virtual/main'
  M V/red-b/e/IV-IV/j/I-II
  M V/red-b/e/IV-IV/j/V
  M V/red-b/e/VIII/cherry
  M V/red-b/e/VIII/grape-II
  M V/red-b/e/VIII/lemon-IV/II
  M V/red-b/e/VIII/lemon-IV/VI
  M V/red-b/e/VIII/pear-II

Sample check a file content:

  $ sl cat -r virtual/main V/red-b/e/IV-IV/j/III-II
  Rabbit, and had just begun to dream that she was walking by the pope, was
  soon submitted to by all three to settle the question, and they drew all
  manner of things-everything that begins with an M, such as mouse-traps, and
  the blades of grass, but she could not remember the simple rules their
  friends had taught them: such as, Sure, I don't see how he can thoroughly
  enjoy The pepper when he finds out who I _was_ when I breathe'!
  
  Don't let me hear the name (no-eol)

Maximum factor_size:

  $ cd
  $ sl init --config format.use-virtual-repo-with-size-factor=34 virtual34
  $ cd virtual34
  $ sl
  o  commit:      e2000000000c
  │  bookmark:    virtual/main
  ~  user:        test <test@example.com>
     date:        Wed Dec 29 08:00:00 9999 +0000
     summary:     synthetic commit 523419074428929
