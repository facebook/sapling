Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

1) Setup configuration
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    hg ci -l msg
  > }


  $ mkdir diagnosis
  $ cd diagnosis
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > fastmanifest=
  > [fastmanifest]
  > logfile=$TESTTMP/logfile
  > EOF


