#chg-compatible

  $ disable treemanifest
#require no-fsmonitor

Show all commands except debug commands
  $ hg debugcomplete
  add
  addremove
  annotate
  archive
  backout
  bisect
  blackbox
  bookmarks
  branch
  bundle
  cat
  clone
  commit
  continue
  copy
  diff
  doctor
  export
  files
  forget
  fs
  githelp
  graft
  grep
  heads
  help
  hint
  histgrep
  identify
  import
  incoming
  init
  locate
  log
  manifest
  merge
  outgoing
  parents
  paths
  phase
  pull
  purge
  push
  record
  recover
  remove
  rename
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
  update
  verify
  version

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
  $ hg debugcomplete debug
  debug-args
  debugancestor
  debugapplystreamclonebundle
  debugbindag
  debugbuilddag
  debugbundle
  debugcapabilities
  debugcauserusterror
  debugchangelog
  debugcheckcasecollisions
  debugcheckoutidentifier
  debugcheckstate
  debugcolor
  debugcommands
  debugcomplete
  debugconfig
  debugcreatestreamclonebundle
  debugdag
  debugdata
  debugdate
  debugdeltachain
  debugdetectissues
  debugdifftree
  debugdirs
  debugdirstate
  debugdiscovery
  debugdrawdag
  debugdumpindexedlog
  debugdumptrace
  debugdynamicconfig
  debugedenimporthelper
  debugedenrunpostupdatehook
  debugexistingcasecollisions
  debugextensions
  debugfilerevision
  debugfileset
  debugformat
  debugfsinfo
  debuggetbundle
  debughttp
  debugignore
  debugindex
  debugindexdot
  debuginstall
  debugknown
  debuglabelcomplete
  debuglocks
  debugmakepublic
  debugmanifestdirs
  debugmergestate
  debugmetalog
  debugmetalogroots
  debugmutation
  debugmutationfromobsmarkers
  debugnamecomplete
  debugobsolete
  debugpathcomplete
  debugpickmergetool
  debugpreviewbindag
  debugprocesstree
  debugprogress
  debugpull
  debugpushkey
  debugpvec
  debugpython
  debugreadauthforuri
  debugrebuilddirstate
  debugrebuildfncache
  debugrename
  debugrevlog
  debugrevspec
  debugrunshell
  debugsendunbundle
  debugsetparents
  debugshell
  debugsmallcommitmetadata
  debugssl
  debugstatus
  debugstore
  debugstrip
  debugsuccessorssets
  debugtemplate
  debugthrowexception
  debugthrowrustbail
  debugthrowrustexception
  debugtreestate
  debugupdatecaches
  debugupgraderepo
  debugvisibility
  debugvisibleheads
  debugwalk
  debugwireargs

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
  --noninteractive
  --outputencoding
  --pager
  --profile
  --quiet
  --repository
  --time
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
  --traceback
  --verbose
  --version
  --web-conf
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
  update checkout co

  $ hg debugcomplete -v
  add
  addremove
  annotate blame
  archive
  backout
  bisect
  blackbox
  bookmarks
  branch
  bundle
  cat
  clone
  commit ci
  continue
  copy cp
  diff
  doctor
  export
  files
  forget
  fs
  githelp
  graft
  grep
  heads
  help
  hint
  histgrep
  identify
  import patch
  incoming
  init
  locate
  log history
  manifest
  merge
  outgoing
  parents
  paths
  phase
  pull
  purge clean
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
  update checkout co
  verify
  version

Show an error if we use --options with an ambiguous abbreviation
  $ hg debugcomplete --options s
  unknown command 's'
  (use 'hg help' to get help)
  [255]

Show all commands + options
  $ hg debugcommands
  add: include, exclude, dry-run
  addremove: similarity, include, exclude, dry-run
  annotate: rev, no-follow, text, user, file, date, number, changeset, line-number, skip, ignore-all-space, ignore-space-change, ignore-blank-lines, ignore-space-at-eol, include, exclude, template
  archive: no-decode, prefix, rev, type, include, exclude
  backout: merge, no-commit, parent, rev, edit, tool, include, exclude, message, logfile, date, user
  bisect: reset, good, bad, skip, extend, command, noupdate, nosparseskip
  blackbox: start, end, pattern, timestamp, sid
  bookmarks: force, rev, delete, strip, rename, inactive, template
  branch: force, clean, new
  bundle: force, rev, base, all, type, ssh, remotecmd, insecure
  cat: output, rev, decode, include, exclude, template
  clone: noupdate, updaterev, rev, pull, uncompressed, stream, shallow, ssh, remotecmd, insecure
  commit: addremove, amend, edit, interactive, reuse-message, include, exclude, message, logfile, date, user
  config: untrusted, edit, local, global, template
  continue: 
  copy: after, force, include, exclude, dry-run
  debug-args: 
  debugancestor: 
  debugapplystreamclonebundle: 
  debugbindag: rev, output
  debugbuilddag: mergeable-file, overwritten-file, new-file
  debugbundle: all, part-type, spec
  debugcapabilities: 
  debugcauserusterror: 
  debugchangelog: migrate
  debugcheckcasecollisions: rev
  debugcheckoutidentifier: 
  debugcheckstate: 
  debugcolor: style
  debugcommands: 
  debugcomplete: options
  debugcreatestreamclonebundle: 
  debugdag: bookmarks, branches, dots, spaces
  debugdata: changelog, manifest, dir
  debugdate: extended, range
  debugdeltachain: changelog, manifest, dir, template
  debugdetectissues: 
  debugdifftree: rev, include, exclude, style, template
  debugdirs: rev, print0
  debugdirstate: nodates, datesort, json
  debugdiscovery: rev, ssh, remotecmd, insecure
  debugdrawdag: print
  debugdumpindexedlog: 
  debugdumptrace: time-range, session-id, output-path
  debugdynamicconfig: canary
  debugedenimporthelper: in-fd, out-fd, manifest, get-manifest-node, cat-file, cat-tree, get-file-size, fetch-tree
  debugedenrunpostupdatehook: 
  debugexistingcasecollisions: rev
  debugextensions: excludedefault, template
  debugfilerevision: rev, include, exclude
  debugfileset: rev
  debugformat: template
  debugfsinfo: 
  debuggetbundle: head, common, type
  debughttp: 
  debugignore: 
  debugindex: changelog, manifest, dir, format
  debugindexdot: changelog, manifest, dir
  debuginstall: template
  debugknown: 
  debuglabelcomplete: 
  debuglocks: force-lock, force-wlock, force-undolog-lock, set-lock, set-wlock
  debugmakepublic: rev, delete
  debugmanifestdirs: rev
  debugmergestate: 
  debugmetalog: time-range
  debugmetalogroots: style, template
  debugmutation: rev, successors, time-range
  debugmutationfromobsmarkers: 
  debugnamecomplete: 
  debugobsolete: flags, record-parents, rev, exclusive, index, delete, date, user, template
  debugpathcomplete: full, normal, added, removed
  debugpickmergetool: rev, changedelete, include, exclude, tool
  debugpreviewbindag: 
  debugprocesstree: 
  debugprogress: spinner, nototal, bytes, sleep, nested, with-output
  debugpull: bookmark, rev
  debugpushkey: 
  debugpvec: 
  debugpython: trace
  debugreadauthforuri: user
  debugrebuilddirstate: rev, minimal
  debugrebuildfncache: 
  debugrename: rev
  debugrevlog: changelog, manifest, dir, dump
  debugrevspec: optimize, show-revs, show-set, show-stage, no-optimized, verify-optimized
  debugrunshell: cmd
  debugsendunbundle: 
  debugsetparents: 
  debugshell: command
  debugsmallcommitmetadata: rev, category, delete, template
  debugssl: 
  debugstatus: nonnormal
  debugstore: content
  debugstrip: rev, force, no-backup, keep, bookmark
  debugsuccessorssets: closest
  debugtemplate: rev, define
  debugthrowexception: 
  debugthrowrustbail: 
  debugthrowrustexception: 
  debugtreestate: 
  debugupdatecaches: 
  debugupgraderepo: optimize, run
  debugvisibility: 
  debugvisibleheads: style, template
  debugwalk: include, exclude
  debugwireargs: three, four, five, ssh, remotecmd, insecure
  diff: rev, change, text, git, binary, nodates, noprefix, show-function, reverse, ignore-all-space, ignore-space-change, ignore-blank-lines, ignore-space-at-eol, unified, stat, root, only-files-in-revs, include, exclude
  doctor: 
  export: output, switch-parent, rev, pattern, text, git, binary, nodates, include, exclude
  files: rev, print0, include, exclude, template
  forget: include, exclude
  fs: 
  githelp: 
  graft: rev, continue, abort, edit, log, force, currentdate, currentuser, date, user, tool, dry-run
  grep: after-context, before-context, context, ignore-case, files-with-matches, line-number, invert-match, word-regexp, extended-regexp, fixed-strings, perl-regexp, include, exclude
  heads: rev, topo, active, closed, style, template
  help: extension, command, keyword, system
  hint: ack
  histgrep: print0, all, text, follow, ignore-case, files-with-matches, line-number, rev, user, date, template, include, exclude
  identify: rev, num, id, branch, tags, bookmarks, ssh, remotecmd, insecure, template
  import: strip, base, edit, force, no-commit, bypass, partial, exact, prefix, message, logfile, date, user, similarity
  incoming: force, newest-first, bundle, rev, bookmarks, patch, git, limit, no-merges, stat, graph, style, template, ssh, remotecmd, insecure
  init: ssh, remotecmd, insecure
  locate: rev, print0, fullpath, include, exclude
  log: follow, follow-first, date, copies, keyword, rev, line-range, removed, only-merges, user, branch, prune, patch, git, limit, no-merges, stat, graph, style, template, include, exclude
  manifest: rev, all, template
  merge: force, rev, preview, tool
  outgoing: force, rev, newest-first, bookmarks, patch, git, limit, no-merges, stat, graph, style, template, ssh, remotecmd, insecure
  parents: rev, style, template
  paths: template
  phase: public, draft, secret, force, rev
  pull: update, force, rev, bookmark, ssh, remotecmd, insecure
  purge: abort-on-err, all, dirs, files, print, print0, include, exclude
  push: force, rev, bookmark, new-branch, pushvars, ssh, remotecmd, insecure
  record: addremove, amend, secret, edit, message, logfile, date, user, ignore-all-space, ignore-space-change, ignore-blank-lines, ignore-space-at-eol, include, exclude
  recover: 
  remove: after, force, include, exclude
  rename: after, force, include, exclude, dry-run
  resolve: all, list, mark, unmark, no-status, root-relative, tool, include, exclude, template, skip
  revert: all, date, rev, no-backup, interactive, include, exclude, dry-run
  rollback: dry-run, force
  root: shared
  serve: accesslog, daemon, daemon-postexec, errorlog, port, address, prefix, name, web-conf, webdir-conf, pid-file, port-file, stdio, cmdserver, templates, style, ipv6, certificate, read-only
  show: nodates, noprefix, stat, git, unified, ignore-all-space, ignore-space-change, ignore-blank-lines, ignore-space-at-eol, style, template, include, exclude
  status: all, modified, added, removed, deleted, clean, unknown, ignored, no-status, terse, copies, print0, rev, change, include, exclude, template
  summary: remote
  tag: force, local, rev, remove, edit, message, date, user
  tags: template
  tip: patch, git, style, template
  unbundle: update
  uncommit: keep, include, exclude
  update: clean, check, merge, date, rev, inactive, continue, tool
  verify: rev
  version: template

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
  default
  fee
  fo
  $ hg debugnamecomplete f
  fee
  fo

Test debuglabelcomplete, a deprecated name for debugnamecomplete that is still
used for completions in some shells.

  $ hg debuglabelcomplete
  Fum
  default
  fee
  fo
  $ hg debuglabelcomplete f
  fee
  fo
