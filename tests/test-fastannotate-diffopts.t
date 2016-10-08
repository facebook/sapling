  $ extpath=`dirname $TESTDIR`
  $ PYTHONPATH=$extpath:$TESTDIR/../:$PYTHONPATH
  $ export PYTHONPATH

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastannotate=
  > EOF

  $ hg init repo
  $ cd repo

changes to whitespaces

  $ cat >> a << EOF
  > 1
  > 
  >  
  >  2
  > EOF
  $ hg commit -qAm '1'
  $ cat > a << EOF
  >  1
  > 
  > 2
  > 
  > 
  > 3
  > EOF
  $ hg commit -m 2
  $ hg fastannotate -wB a
  0:  1
  0: 
  1: 2
  0: 
  1: 
  1: 3
