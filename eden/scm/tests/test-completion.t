#debugruntest-compatible


#require no-fsmonitor

Show all commands except debug commands
  $ hg debugcomplete | grep 'commit|diff|status|debugapi'
  commit
  diff
  status
  uncommit

Show all commands that start with "a"
  $ hg debugcomplete a
  add
  addremove
  annotate
  archive

Do not show debug commands if there are other candidates
  $ hg debugcomplete d
  diff
  doctor

Show debug commands if there are no other candidates
  $ hg debugcomplete debug | grep 'debugapi|debugshell'
  debugapi
  debugshell

Do not show the alias of a debug command if there are other candidates
(this should hide rawcommit)
  $ hg debugcomplete r
  record
  recover
  remove
  rename
  resolve
  revert
  rollback
  root
Show the alias of a debug command if there are no other candidates
  $ hg debugcomplete rawc
  

Show the global options
  $ hg debugcomplete --options | LC_ALL=C sort
  --color
  --config
  --configfile
  --cwd
  --debug
  --debugger
  --encoding
  --encodingmode
  --help
  --hidden
  --insecure
  --noninteractive
  --outputencoding
  --pager
  --profile
  --quiet
  --repository
  --time
  --trace
  --traceback
  --verbose
  --version
  -R
  -h
  -q
  -v
  -y

Show the options for the "serve" command
  $ hg debugcomplete --options serve | LC_ALL=C sort
  --accesslog
  --address
  --certificate
  --cmdserver
  --color
  --config
  --configfile
  --cwd
  --daemon
  --daemon-postexec
  --debug
  --debugger
  --encoding
  --encodingmode
  --errorlog
  --help
  --hidden
  --insecure
  --ipv6
  --name
  --noninteractive
  --outputencoding
  --pager
  --pid-file
  --port
  --port-file
  --prefix
  --profile
  --quiet
  --read-only
  --repository
  --stdio
  --style
  --templates
  --time
  --trace
  --traceback
  --verbose
  --version
  -6
  -A
  -E
  -R
  -a
  -d
  -h
  -n
  -p
  -q
  -t
  -v
  -y

Show aliases with -v
  $ hg debugcomplete update -v
  update checkout co goto

  $ hg debugcomplete -v
  add
  addremove addrm
  annotate blame
  archive
  backout
  bisect
  blackbox
  bookmark bookmarks
  branch
  bundle
  cat
  clean purge
  clone
  commit ci
  configfile
  continue
  copy cp
  diff
  doctor
  export
  files
  forget
  fs
  gc
  githelp
  graft
  grep
  heads
  help
  hint
  histgrep
  identify
  import patch
  init
  locate
  log history
  manifest
  merge
  parents
  paths
  phase
  prefetch
  pull
  push
  record
  recover
  remove rm
  rename move mv
  resolve
  revert
  rollback
  root
  serve
  show
  status
  summary
  tag
  tags
  tip
  unbundle
  uncommit
  uncopy
  update checkout co goto
  verify
  verifyremotefilelog
  version
  web isl
  whereami

Show an error if we use --options with an ambiguous abbreviation
  $ hg debugcomplete --options s
  unknown command 's'
  (use 'hg help' to get help)
  [255]

Show all commands + options
  $ hg debugcommands | grep 'cat:|debugcolor:'
  cat: output, rev, decode, include, exclude, template
  debugcolor: style

  $ hg init a
  $ cd a
  $ echo fee > fee
  $ hg ci -q -Amfee
  $ hg book fee
  $ mkdir fie
  $ echo dead > fie/dead
  $ echo live > fie/live
  $ hg bookmark fo
  $ hg ci -q -Amfie
  $ echo fo > fo
  $ hg ci -q -Amfo
  $ echo Fum > Fum
  $ hg ci -q -AmFum
  $ hg bookmark Fum

Test debugpathcomplete

  $ hg debugpathcomplete f
  fee
  fie
  fo
  $ hg debugpathcomplete -f f
  fee
  fie/dead
  fie/live
  fo

  $ hg rm Fum
  $ hg debugpathcomplete -r F
  Fum

Test debugnamecomplete

  $ hg debugnamecomplete
  Fum
  fee
  fo
  $ hg debugnamecomplete f
  fee
  fo
  $ hg debugnamecomplete --config zsh.completion-description=true --description
  Fum:Fum
  fee:fee
  fo:Fum

Test debuglabelcomplete, a deprecated name for debugnamecomplete that is still
used for completions in some shells.

  $ hg debuglabelcomplete
  Fum
  fee
  fo
  $ hg debuglabelcomplete f
  fee
  fo
