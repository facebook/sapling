#require p4
  $ . $TESTDIR/p4setup.sh

Create changelist with file contaning non-ascii character in name
  $ echo a > á.txt
  $ p4 -q add -t text á.txt
  $ p4 -q submit -d 'ááa'

Setup hg repo
  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport -q -P $P4ROOT -B master $P4CLIENT --limit 1

End Test
  stopping the p4 server
