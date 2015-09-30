  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/automv.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > automv=$TESTTMP/automv.py
  > fbamend=$extpath/fbamend.py
  > rebase=
  > EOF

Setup repo

  $ hg init repo
  $ cd repo

Test automv command for commit

  $ echo 'foo' > a.txt
  $ hg add a.txt
  $ hg commit -m 'init repo with a'

mv/rm/add
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --config automv.testmode=true
  detected move of 1 file
  $ hg status -C
  A b.txt
    a.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt

mv/rm/add/modif
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ echo $'\n' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --config automv.testmode=true
  detected move of 1 file
  $ hg status -C
  A b.txt
    a.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt

mv/rm/add/modif
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ echo $'\nfoo' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --config automv.testmode=true
  $ hg status -C
  A b.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt

mv/rm/add/modif/changethreshold
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ echo $'\nfoo' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --config automv.testmode=true --config automv.similaritythres='0.6'
  detected move of 1 file
  $ hg status -C
  A b.txt
    a.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt

mv
  $ mv a.txt b.txt
  $ hg status -C
  ! a.txt
  ? b.txt
  $ hg commit --config automv.testmode=true
  $ hg status -C
  ! a.txt
  ? b.txt
  $ hg revert -aqC
  $ rm b.txt

mv/rm/add/notincommitfiles
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ echo 'bar' > c.txt
  $ hg add c.txt
  $ hg status -C
  A b.txt
  A c.txt
  R a.txt
  $ hg commit --config automv.testmode=true c.txt
  $ hg status -C
  A b.txt
  A c.txt
  R a.txt
  $ hg commit --config automv.testmode=true
  detected move of 1 file
  $ hg status -C
  A b.txt
    a.txt
  A c.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt
  $ rm c.txt

mv/rm/add/--no-move-detection
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg commit --no-move-detection --config automv.testmode=true
  $ hg status -C
  A b.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt


Test automv command for amend

mv/rm/add
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg amend --config automv.testmode=true
  detected move of 1 file
  $ hg status -C
  A b.txt
    a.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt

mv/rm/add/modif
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ echo $'\n' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg amend --config automv.testmode=true
  detected move of 1 file
  $ hg status -C
  A b.txt
    a.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt

mv/rm/add/modif
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ echo $'\nfoo' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg amend --config automv.testmode=true
  $ hg status -C
  A b.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt

mv/rm/add/modif/changethreshold
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ echo $'\nfoo' >> b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg amend --config automv.testmode=true --config automv.similaritythres='0.6'
  detected move of 1 file
  $ hg status -C
  A b.txt
    a.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt

mv
  $ mv a.txt b.txt
  $ hg status -C
  ! a.txt
  ? b.txt
  $ hg amend --config automv.testmode=true
  $ hg status -C
  ! a.txt
  ? b.txt
  $ hg revert -aqC
  $ rm b.txt

mv/rm/add/notincommitfiles
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ echo 'bar' > c.txt
  $ hg add c.txt
  $ hg status -C
  A b.txt
  A c.txt
  R a.txt
  $ hg amend --config automv.testmode=true c.txt
  $ hg status -C
  A b.txt
  A c.txt
  R a.txt
  $ hg amend --config automv.testmode=true
  detected move of 1 file
  $ hg status -C
  A b.txt
    a.txt
  A c.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt
  $ rm c.txt

mv/rm/add/--no-move-detection
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg add b.txt
  $ hg status -C
  A b.txt
  R a.txt
  $ hg amend --no-move-detection --config automv.testmode=true
  $ hg status -C
  A b.txt
  R a.txt
  $ hg revert -aqC
  $ rm b.txt


mv/rm/commit/add/amend
  $ mv a.txt b.txt
  $ hg rm a.txt
  $ hg status -C
  R a.txt
  ? b.txt
  $ hg commit -m "removed a"
  $ hg add b.txt
  $ hg amend --config automv.testmode=true
  $ hg status -C
  A b.txt
