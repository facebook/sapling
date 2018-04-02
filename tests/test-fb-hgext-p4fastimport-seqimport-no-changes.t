#require p4

  $ . $TESTDIR/p4setup.sh
  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'

Abort on no changes to import

  $ hg p4seqimport --debug -B master -P $P4ROOT $P4CLIENT
  loading changelist numbers.
  0 changelists to import.
  no changes to import, exiting.

End Test
  stopping the p4 server
