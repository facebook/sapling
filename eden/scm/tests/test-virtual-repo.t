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

Total file count and size is reasonable:

  $ sl go 'roots(all())' -q
  $ sl files | wc -l
  6084
  >>> import glob, os
  >>> size = sum(os.lstat(path).st_size for path in glob.glob('**/*', recursive=True))
  >>> print(size)
  79154496

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
