#chg-compatible
#debugruntest-compatible

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
  bookmark
  branch
  bundle
  cat
  clean
  clone
  commit
  configfile
  continue
  copy
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
  import
  init
  locate
  log
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
  remove
  rename
  repack
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
  update
  verify
  verifyremotefilelog
  version
  web
  whereami

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
  debugapi
  debugapplystreamclonebundle
  debugbenchmarkrevsets
  debugbindag
  debugbuilddag
  debugbundle
  debugcapabilities
  debugchangelog
  debugcheckcasecollisions
  debugcheckoutidentifier
  debugcheckstate
  debugcleanremotenames
  debugcolor
  debugcommands
  debugcompactmetalog
  debugcomplete
  debugconfig
  debugcreatestreamclonebundle
  debugdag
  debugdata
  debugdatapack
  debugdate
  debugdeltachain
  debugdetectissues
  debugdiffdirs
  debugdifftree
  debugdirs
  debugdirstate
  debugdiscovery
  debugdrawdag
  debugdryup
  debugdumpdynamicconfig
  debugdumpindexedlog
  debugdumptrace
  debugduplicatedconfig
  debugdynamicconfig
  debugedenimporthelper
  debugedenrunpostupdatehook
  debugexistingcasecollisions
  debugexportmetalog
  debugexportrevlog
  debugextensions
  debugfilerevision
  debugfileset
  debugfsinfo
  debugfsync
  debuggetbundle
  debuggetroottree
  debughistorypack
  debughttp
  debugignore
  debugindex
  debugindexdot
  debugindexedlogdatastore
  debugindexedloghistorystore
  debuginitgit
  debuginstall
  debuginternals
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
  debugnetworkdoctor
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
  debugracyoutput
  debugreadauthforuri
  debugrebuildchangelog
  debugrebuilddirstate
  debugrebuildfncache
  debugremotefilelog
  debugrename
  debugresetheads
  debugrevlog
  debugrevlogclone
  debugrevset
  debugrevspec
  debugrunlog
  debugrunshell
  debugruntest
  debugscmstore
  debugscmstorereplay
  debugsegmentclone
  debugsegmentgraph
  debugsegmentpull
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
  debugtop
  debugtreestate
  debugupdatecaches
  debugvisibility
  debugvisibleheads
  debugwaitonprefetch
  debugwaitonrepack
  debugwalk
  debugwireargs

Do not show the alias of a debug command if there are other candidates
(this should hide rawcommit)
  $ hg debugcomplete r
  record
  recover
  remove
  rename
  repack
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
  repack
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
  $ hg debugcommands
  add: include, exclude, dry-run
  addremove: similarity, include, exclude, dry-run
  annotate: rev, no-follow, text, user, file, date, number, changeset, line-number, skip, short-date, ignore-all-space, ignore-space-change, ignore-blank-lines, ignore-space-at-eol, include, exclude, template
  archive: no-decode, prefix, rev, type, include, exclude
  backout: merge, no-commit, parent, rev, edit, tool, include, exclude, message, logfile, date, user
  bisect: reset, good, bad, skip, extend, command, noupdate, nosparseskip
  blackbox: start, end, pattern, timestamp, sid
  bookmark: force, rev, delete, strip, rename, inactive, template
  branch: force, clean, new
  bundle: force, rev, base, all, type
  cat: output, rev, decode, include, exclude, template
  clean: abort-on-err, all, ignored, dirs, files, print, print0, include, exclude
  clone: noupdate, updaterev, rev, pull, stream, shallow, git
  commit: addremove, amend, edit, interactive, reuse-message, include, exclude, message, logfile, date, user
  configfile: user, local, system
  config: edit, user, local, system, global, template
  continue: 
  copy: after, force, include, exclude, dry-run
  debug-args: 
  debugancestor: 
  debugapi: endpoint, input, input-file, sort
  debugapplystreamclonebundle: 
  debugbenchmarkrevsets: rev-x, rev-y, expr, default, multi-backend
  debugbindag: rev, output
  debugbuilddag: mergeable-file, overwritten-file, new-file
  debugbundle: all, part-type, spec
  debugcapabilities: 
  debugchangelog: migrate, unless, remove-backup
  debugcheckcasecollisions: rev
  debugcheckoutidentifier: 
  debugcheckstate: 
  debugcleanremotenames: 
  debugcolor: style
  debugcommands: 
  debugcompactmetalog: 
  debugcomplete: options
  debugcreatestreamclonebundle: 
  debugdag: bookmarks, branches, dots, spaces
  debugdata: changelog, manifest, dir
  debugdatapack: long, node, node-delta
  debugdate: extended, range
  debugdeltachain: changelog, manifest, dir, template
  debugdetectissues: 
  debugdiffdirs: rev, include, exclude, style, template
  debugdifftree: rev, include, exclude, style, template
  debugdirs: rev, print0
  debugdirstate: nodates, datesort, json
  debugdiscovery: rev
  debugdrawdag: print, bookmarks, files, write-env
  debugdryup: 
  debugdumpdynamicconfig: reponame, username, canary
  debugdumpindexedlog: 
  debugdumptrace: time-range, session-id, output-path
  debugduplicatedconfig: style, template
  debugdynamicconfig: canary
  debugedenimporthelper: in-fd, out-fd, manifest, get-manifest-node, cat-file, cat-tree, get-file-size, fetch-tree
  debugedenrunpostupdatehook: 
  debugexistingcasecollisions: rev
  debugexportmetalog: 
  debugexportrevlog: 
  debugextensions: excludedefault, template
  debugfilerevision: rev, include, exclude
  debugfileset: rev
  debugfsinfo: 
  debugfsync: 
  debuggetbundle: head, common, type
  debuggetroottree: 
  debughistorypack: long
  debughttp: 
  debugignore: 
  debugindex: changelog, manifest, dir, format
  debugindexdot: changelog, manifest, dir
  debugindexedlogdatastore: long, node, node-delta
  debugindexedloghistorystore: long
  debuginitgit: git-dir
  debuginstall: template
  debuginternals: output
  debugknown: 
  debuglabelcomplete: 
  debuglocks: force-lock, force-wlock, force-undolog-lock, set-lock, set-wlock, wait
  debugmakepublic: rev, delete
  debugmanifestdirs: rev
  debugmergestate: 
  debugmetalog: time-range
  debugmetalogroots: style, template
  debugmutation: rev, successors, time-range
  debugmutationfromobsmarkers: 
  debugnamecomplete: 
  debugnetworkdoctor: 
  debugobsolete: flags, record-parents, rev, exclusive, index, date, user, template
  debugpathcomplete: full, normal, added, removed
  debugpickmergetool: rev, changedelete, include, exclude, tool
  debugpreviewbindag: 
  debugprocesstree: 
  debugprogress: spinner, nototal, bytes, sleep, nested, with-output
  debugpull: bookmark, rev
  debugpushkey: 
  debugpvec: 
  debugpython: 
  debugracyoutput: time-series, progress-bars, progress-total, progress-interval-ms, output-total, output-interval-ms
  debugreadauthforuri: user
  debugrebuildchangelog: revlog
  debugrebuilddirstate: rev, minimal
  debugrebuildfncache: 
  debugremotefilelog: decompress
  debugrename: rev
  debugresetheads: 
  debugrevlog: changelog, manifest, dir, dump
  debugrevlogclone: 
  debugrevset: 
  debugrevspec: optimize, show-revs, show-set, show-stage, no-optimized, verify-optimized
  debugrunlog: ended, template
  debugrunshell: cmd
  debugruntest: fix, jobs, ext, direct
  debugscmstore: mode, path, python
  debugscmstorereplay: path
  debugsegmentclone: 
  debugsegmentgraph: level, group
  debugsegmentpull: 
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
  debugtop: refresh-rate, reap-delay, columns
  debugtreestate: 
  debugupdatecaches: 
  debugvisibility: 
  debugvisibleheads: style, template
  debugwaitonprefetch: 
  debugwaitonrepack: 
  debugwalk: include, exclude
  debugwireargs: three, four, five
  diff: rev, change, text, git, binary, nodates, noprefix, show-function, reverse, ignore-all-space, ignore-space-change, ignore-blank-lines, ignore-space-at-eol, unified, stat, root, only-files-in-revs, include, exclude
  doctor: 
  export: output, switch-parent, rev, pattern, text, git, binary, nodates, include, exclude
  files: rev, print0, include, exclude, template
  forget: include, exclude
  fs: 
  gc: 
  githelp: 
  graft: rev, continue, abort, edit, log, force, currentdate, currentuser, date, user, tool, dry-run
  grep: after-context, before-context, context, ignore-case, files-with-matches, line-number, invert-match, word-regexp, extended-regexp, fixed-strings, perl-regexp, include, exclude
  heads: rev, topo, active, closed, style, template
  help: extension, command, keyword, system
  hint: ack
  histgrep: print0, all, text, follow, ignore-case, files-with-matches, line-number, rev, user, date, template, include, exclude
  identify: rev, num, id, branch, tags, bookmarks, template
  import: strip, base, edit, force, no-commit, bypass, partial, exact, prefix, message, logfile, date, user, similarity
  init: git
  locate: rev, print0, fullpath, include, exclude
  log: follow, follow-first, date, copies, keyword, rev, line-range, removed, only-merges, user, branch, prune, patch, git, limit, no-merges, stat, graph, style, template, include, exclude
  manifest: rev, all, template
  merge: force, rev, preview, tool
  parents: rev, style, template
  paths: template
  phase: public, draft, secret, force, rev
  prefetch: rev, repack, base, include, exclude
  pull: update, force, rev, bookmark
  push: force, rev, bookmark, new-branch, pushvars
  record: addremove, amend, secret, edit, message, logfile, date, user, ignore-all-space, ignore-space-change, ignore-blank-lines, ignore-space-at-eol, include, exclude
  recover: 
  remove: after, force, include, exclude
  rename: after, force, include, exclude, dry-run
  repack: background, incremental
  resolve: all, list, mark, unmark, no-status, root-relative, tool, include, exclude, template, skip
  revert: all, date, rev, no-backup, interactive, include, exclude, dry-run
  rollback: dry-run, force
  root: shared, dotdir
  serve: accesslog, daemon, daemon-postexec, errorlog, port, address, prefix, name, pid-file, port-file, stdio, cmdserver, templates, style, ipv6, certificate, read-only
  show: nodates, noprefix, stat, git, unified, ignore-all-space, ignore-space-change, ignore-blank-lines, ignore-space-at-eol, style, template, include, exclude
  status: all, modified, added, removed, deleted, clean, unknown, ignored, no-status, terse, copies, print0, rev, change, include, exclude, template
  summary: remote
  tag: force, local, rev, remove, edit, message, date, user
  tags: template
  tip: patch, git, style, template
  unbundle: update
  uncommit: keep, include, exclude
  uncopy: include, exclude, dry-run
  update: clean, check, merge, date, rev, inactive, continue, tool
  verify: rev, dag
  verifyremotefilelog: decompress
  version: template
  web: port, json, open, foreground, kill, force, platform
  whereami: 

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
