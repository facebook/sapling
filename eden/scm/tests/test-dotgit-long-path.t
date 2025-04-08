#require git no-eden windows

  $ . $TESTDIR/git.sh

Test long path for the git store.
Right now, libgit2 does not support long path store. See:
- https://github.com/libgit2/libgit2/blob/9903482593db438abbbbaf5324a0cc78c5472603/docs/win32-longpaths.md
- https://github.com/libgit2/libgit2/issues/6604

  $ LONG=this-is-a-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-path
  $ mkdir -p $LONG
  $ cd $LONG

  $ git init -q -b main repo
  $ cd repo
  $ touch a
  $ RUST_BACKTRACE=0 sl commit -m 'Add a' -A a
  abort: When constructing alloc::boxed::Box<dyn storemodel::StoreOutput> from dyn storemodel::StoreInfo, after being ignored by ["eager"], "git" reported error
  
  Caused by:
      0: opening git store
      1: path too long: '$TESTTMP/this-is-a-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-long-path/repo/.git/'; class=Filesystem (30)
  [255]
