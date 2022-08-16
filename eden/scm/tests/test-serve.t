#chg-compatible
#debugruntest-compatible

#require serve

  $ hg init test
  $ cd test
  $ hg serve -a localhost -p 0 --port-file $TESTTMP/.port -d --pid-file=hg.pid -E errors.log
  abort: hgweb is deprecated and services should stop using it
  (set `--config web.allowhgweb=True` to bypass the block temporarily, but this will be going away soon)
  [255]
