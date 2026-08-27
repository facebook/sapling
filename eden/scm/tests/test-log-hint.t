
#require no-eden


  $ setconfig tweakdefaults.logdefaultfollow=true

  $ newclientrepo
  $ drawdag <<EOS
  > B
  > |
  > A
  > EOS
  $ sl go -q $B

  $ sl log tip
  abort: cannot follow file not in parent revision: "tip"
  (did you mean "sl log -r 'tip'", or "sl log -r 'tip' -f" to follow history?)
  [255]

  $ sl log -r 'tip'
  commit:      112478962961
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B

  $ sl log -r 'tip' -f
  commit:      112478962961
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B
  
  commit:      426bada5c675
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     A

Hint when a cwd-relative path typo matches a path from the repo root:

  $ newrepo

  $ drawdag << 'EOS'
  > B  # B/C=A (renamed from A)
  > |
  > A
  > EOS

  $ sl go -q $B
  $ mkdir sub
  $ cd sub

  $ sl log C -T '{desc}\n'
  hint[rel-path-typo]: path 'C' does not exist relative to the current directory
   (use 'path:C' to specify the matching repo-root-relative path)
  abort: cannot follow file not in parent revision: "sub/C"
  [255]

  $ sl log path:C -T '{desc}\n'
  B
  A

  $ sl log -r "$A" A -T '{desc}\n'
  hint[rel-path-typo]: path 'A' does not exist relative to the current directory
   (use 'path:A' to specify the matching repo-root-relative path)
