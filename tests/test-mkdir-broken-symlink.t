#require symlink

  $ mkdir -p a
  $ ln -s a/b a/c
  $ hg debugshell -c 'm.util.makedirs("a/c/e/f")'
  abort: Symlink '$TESTTMP/a/c' points to non-existed destination 'a/b' during makedir: '$TESTTMP/a/c/e'
  [255]
