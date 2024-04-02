#debugruntest-compatible
#require fsmonitor no-eden

  $ configure modernclient
  $ newclientrepo
  $ echo foo > foo
  $ hg commit -Aqm foo

  $ echo nope > $TESTTMP/watchman
  $ chmod +x $TESTTMP/watchman
  $ export PATH=$TESTTMP:$PATH
  $ unset WATCHMAN_SOCK

  $ echo foo >> foo
  $ LOG=warn,watchman_info=debug hg st --config fsmonitor.fallback-on-watchman-exception=true
  DEBUG watchman_info: watchmanfallback=1
   WARN workingcopy::filesystem::watchmanfs::watchmanfs: watchman error - falling back to slow crawl * (glob)
  ` (?)
  M foo

  $ LOG=warn,watchman_info=debug hg st --config fsmonitor.fallback-on-watchman-exception=false
  abort: While invoking the watchman CLI * (glob)
  ` (?)
  [255]
