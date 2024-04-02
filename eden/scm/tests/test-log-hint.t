#debugruntest-compatible

#require no-eden


  $ configure modernclient
  $ setconfig tweakdefaults.logdefaultfollow=true

  $ newclientrepo
  $ drawdag <<EOS
  > B
  > |
  > A
  > EOS
  $ hg go -q $B

  $ hg log tip
  abort: cannot follow file not in parent revision: "tip"
  (did you mean "hg log -r 'tip'", or "hg log -r 'tip' -f" to follow history?)
  [255]

  $ hg log -r 'tip'
  commit:      112478962961
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B

  $ hg log -r 'tip' -f
  commit:      112478962961
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B
  
  commit:      426bada5c675
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     A
