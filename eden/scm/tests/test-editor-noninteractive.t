  $ eagerepo

interactive editor should abort in non-interactive mode
  $ newrepo
  $ echo foo > foo
  $ hg add foo
  $ hg st
  A foo
  $ HGEDITOR="echo hello >" hg commit --config experimental.allow-non-interactive-editor=false
  abort: cannot start editor in non-interactive mode to complete the 'commit' action
  (consider running 'commit' action from the command line)
  [255]
  $ HGEDITOR="echo hello >" hg commit --config experimental.allow-non-interactive-editor=true
  $ hg log -T "{desc}\n" -r .
  hello
  $ echo bar >> foo
  $ hg st
  M foo
  $ HGEDITOR="echo bar >" hg commit
  $ hg log -T "{desc}\n" -r .
  bar
