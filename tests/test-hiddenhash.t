  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/hiddenhash.py $TESTTMP
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > hiddenhash=$TESTTMP/hiddenhash.py
  > [experimental]
  > evolution=all
  > EOF
  $ hg init repo && cd repo
  $ hg debugbuilddag +1
  $ hg debugobsolete 1ea73414a91b0920940797d8fc6a11e447f8ea1e
  $ hg log -r 0
  abort: hidden changeset 1ea73414a91b!
  [255]
