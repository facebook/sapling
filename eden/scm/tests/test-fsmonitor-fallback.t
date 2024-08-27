#require fsmonitor no-eden

  $ newclientrepo
  $ echo foo > foo
  $ hg commit -Aqm foo

  $ echo nope > $TESTTMP/watchman
  $ chmod +x $TESTTMP/watchman
  $ export PATH=$TESTTMP:$PATH
  $ unset WATCHMAN_SOCK

  $ echo foo >> foo
  $ LOG=warn,watchman_info=debug hg st --config fsmonitor.fallback-on-watchman-exception=true
   WARN configloader::hg: repo name: no remotefilelog.reponame
  DEBUG watchman_info: watchmanfallback=1
   WARN workingcopy::filesystem::watchmanfs::watchmanfs: watchman error - falling back to slow crawl * (glob)
  ` (?)
  M foo

  $ LOG=warn,watchman_info=debug hg st --config fsmonitor.fallback-on-watchman-exception=false
   WARN configloader::hg: repo name: no remotefilelog.reponame
  abort: While invoking the watchman CLI * (glob)
  ` (?)
  [255]
