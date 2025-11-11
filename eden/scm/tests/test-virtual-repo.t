  $ enable smartlog
  $ sl init --config format.use-virtual-repo-with-size-factor=2 virtual1
  $ cd virtual1
  $ sl
  o  commit:      c20cdc010000
  │  bookmark:    virtual/main
  ~  user:        test <test@example.com>
     date:        Sat Oct 25 09:35:59 2025 +0000
     summary:     synthetic commit 121869
  $ sl go 'roots(all())' -q
  $ sl files | wc -l
  6084
  >>> import glob, os
  >>> size = sum(os.lstat(path).st_size for path in glob.glob('**/*', recursive=True))
  >>> print(size)
  79154496
