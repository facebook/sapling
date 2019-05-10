#require p4

  $ . $TESTDIR/p4setup.sh
  $ cat >> $HGRCPATH<<EOF
  > [extensions]
  > lfs=
  > EOF

Create first CL
  $ mkdir Main
  $ echo first > Main/first
  $ p4 -q add Main/first
  $ p4 -q submit -d first

Import first CL
  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport -P $P4ROOT -B master $P4CLIENT --traceback

Commit directly to hg
  $ hg update -q master
  $ echo second > Main/second
  $ hg commit -Aqm 'second'

Create second CL
  $ cd $p4wd
  $ echo third > Main/third
  $ p4 -q add Main/third
  $ p4 -q submit -d third

Import second CL (third commit)
  $ cd $hgwd
  $ hg update -q 000000
  $ hg p4seqimport -P $P4ROOT -B master $P4CLIENT --traceback

Confirm third was committed on top of second
  $ hg log -r '::master' -T '{desc}\n'
  first
  second
  third

End Test
  stopping the p4 server
