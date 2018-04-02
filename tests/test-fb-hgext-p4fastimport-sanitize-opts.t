#require p4

  $ . $TESTDIR/p4setup.sh
  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'

Verifies limit is a positive integer

  $ hg p4seqimport --limit -10 -P $P4ROOT $P4CLIENT
  abort: --limit should be > 0, got -10
  [255]
  $ hg p4seqimport --limit foo -P $P4ROOT $P4CLIENT
  abort: --limit should be an integer, got foo
  [255]

Ensures --base requests bookmark to be provided

  $ hg p4seqimport --base 10 -P $P4ROOT $P4CLIENT
  abort: must set --bookmark when using --base
  [255]

End Test

  stopping the p4 server
