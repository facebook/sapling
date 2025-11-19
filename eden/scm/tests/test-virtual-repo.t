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
  ...         total_size += st.st_size
  ...     return len(paths), total_size

  >>> check_count_file_size()
  (6084, 78988056)

Checkout virtual/main with more files (~800MB):

  $ sl go 'virtual/main' -q
  $ sl files > $FILES

  >>> check_count_file_size()
  (57364, 793475905)

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

  $ sl cat -r virtual/main V/red-b/e/IV-IV/j/V
  She is such a puzzled expression that she had read several nice little
  histories about children who had been all the arches are gone from this side
  of the party went back to yesterday, because I was a long tail, certainly,
  said Alice, a little hot tea upon its forehead (the position in which the
  words DRINK ME, beautifully printed on it except a little of the trees under
  which she had known them all her coaxing.
  
  Puss, she began, rather timidly, saying to  (no-eol)
