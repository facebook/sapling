  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/tweakdefaults.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=$TESTTMP/tweakdefaults.py
  > rebase=
  > EOF

Test histgrep and check that it respects the specified file
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
  abort: cannot run histgrep on the whole repo, please provide filenames
  (this is disabled to avoid insanely slow greps over the whole repo)
  [255]
  $ cd ..
