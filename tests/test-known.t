  $ "$TESTDIR/hghave" killdaemons || exit 80

= Test the known() protocol function =

Create a test repository:

  $ hg init repo
  $ cd repo
  $ touch a ; hg add a ; hg ci -ma
  $ touch b ; hg add b ; hg ci -mb
  $ touch c ; hg add c ; hg ci -mc
  $ hg log --template '{node}\n'
  991a3460af53952d10ec8a295d3d2cc2e5fa9690
  0e067c57feba1a5694ca4844f05588bb1bf82342
  3903775176ed42b1458a6281db4a0ccf4d9f287a
  $ cd ..

Test locally:

  $ hg debugknown repo 991a3460af53952d10ec8a295d3d2cc2e5fa9690 0e067c57feba1a5694ca4844f05588bb1bf82342 3903775176ed42b1458a6281db4a0ccf4d9f287a
  111
  $ hg debugknown repo 000a3460af53952d10ec8a295d3d2cc2e5fa9690 0e067c57feba1a5694ca4844f05588bb1bf82342 0003775176ed42b1458a6281db4a0ccf4d9f287a
  010
  $ hg debugknown repo
  

Test via HTTP:

  $ hg serve -R repo -p $HGPORT -d --pid-file=hg.pid -E error.log -A access.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg debugknown http://localhost:$HGPORT/ 991a3460af53952d10ec8a295d3d2cc2e5fa9690 0e067c57feba1a5694ca4844f05588bb1bf82342 3903775176ed42b1458a6281db4a0ccf4d9f287a
  111
  $ hg debugknown http://localhost:$HGPORT/ 000a3460af53952d10ec8a295d3d2cc2e5fa9690 0e067c57feba1a5694ca4844f05588bb1bf82342 0003775176ed42b1458a6281db4a0ccf4d9f287a
  010
  $ hg debugknown http://localhost:$HGPORT/
  
  $ cat error.log
  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS

