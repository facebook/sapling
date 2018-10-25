Simulate an environment that disables allowfullrepogrep:
  $ setconfig histgrep.allowfullrepogrep=False

Test histgrep and check that it respects the specified file:
  $ hg init repo
  $ cd repo
  $ mkdir histgrepdir
  $ cd histgrepdir
  $ echo 'ababagalamaga' > histgrepfile1
  $ echo 'ababagalamaga' > histgrepfile2
  $ hg add histgrepfile1
  $ hg add histgrepfile2
  $ hg commit -m "Added some files"
  $ hg histgrep ababagalamaga histgrepfile1
  histgrepdir/histgrepfile1:0:ababagalamaga
  $ hg histgrep ababagalamaga
  abort: can't run histgrep on the whole repo, please provide filenames
  (this is disabled to avoid very slow greps over the whole repo)
  [255]

Now allow allowfullrepogrep:
  $ setconfig histgrep.allowfullrepogrep=True
  $ hg histgrep ababagalamaga
  histgrepdir/histgrepfile1:0:ababagalamaga
  histgrepdir/histgrepfile2:0:ababagalamaga
  $ cd ..

