#require test-repo

  $ . $TESTDIR/require-core-hg.sh contrib/check-config.py

This file is backported from mercurial/tests/test-check-config.t.
It differs slightly to fix paths and include files in core hg.

  $ . "$TESTDIR/helper-testrepo.sh"

New errors are not allowed. Warnings are strongly discouraged.

  $ export RUNTESTDIR
  $ (
  >   hg files "set:(**.py or **.txt) - tests/**"
  >   hg files --cwd $RUNTESTDIR/.. "set:(**.py or **.txt) - tests/**" | sed "s#^#${RUNTESTDIR}/../#"
  > ) | sed 's|\\|/|g' |
  >   $PYTHON $RUNTESTDIR/../contrib/check-config.py
  undocumented: extensions.treemanifest (str)
  undocumented: fastlog.debug (str)
  undocumented: fastlog.enabled (bool)
  undocumented: fastmanifest.cachecutoffdays (int) [60]
  undocumented: fastmanifest.cacheonchange (bool)
  undocumented: fastmanifest.cacheonchangebackground (bool) [True]
  undocumented: fastmanifest.debugfastmanifest (bool)
  undocumented: fastmanifest.debugmetrics (bool)
  undocumented: fastmanifest.logfile (str)
  undocumented: fastmanifest.maxinmemoryentries (str) [DEFAULT_MAX_MEMORY_ENTRIES]
  undocumented: fastmanifest.silent (bool)
  undocumented: fastmanifest.usecache (bool)
  undocumented: fastmanifest.usetree (bool)
  undocumented: fbconduit.backingrepos (list) [[reponame]]
  undocumented: fbconduit.gitcallsigns (list)
  undocumented: fbconduit.host (str)
  undocumented: fbconduit.path (str)
  undocumented: fbconduit.protocol (str)
  undocumented: fbconduit.reponame (str)
  undocumented: fbhistedit.exec_in_user_shell (str)
  undocumented: grep.command (str)
  undocumented: infinitepush.bundle-stream (bool)
  undocumented: morestatus.show (bool)
  undocumented: nointerrupt.interactiveonly (bool) [True]
  undocumented: perftweaks.cachenoderevs (bool) [True]
  undocumented: perftweaks.disablebranchcache (bool)
  undocumented: perftweaks.disablecasecheck (bool)
  undocumented: perftweaks.disabletags (bool)
  undocumented: perftweaks.preferdeltas (bool)
  undocumented: phabricator.graphql_app_id (str)
  undocumented: phabricator.graphql_app_token (str)
  undocumented: phabricator.graphql_host (str)
  undocumented: phabstatus.logpeekahead (int) [30]
  undocumented: phrevset.callsign (str)
  undocumented: pushrebase.blocknonpushrebase (bool)
  undocumented: pushrebase.rewritedates (bool)
  undocumented: rage.fastmanifestcached (bool)
  undocumented: remotefilelog.backgroundrepack (bool)
  undocumented: remotefilelog.cachegroup (str)
  undocumented: remotefilelog.debug (bool)
  undocumented: remotefilelog.excludepattern (list)
  undocumented: remotefilelog.fastdatapack (bool)
  undocumented: remotefilelog.fetchpacks (bool)
  undocumented: remotefilelog.fetchwarning (str)
  undocumented: remotefilelog.getfilesstep (int) [10000]
  undocumented: remotefilelog.getfilestype (str) ['optimistic']
  undocumented: remotefilelog.includepattern (list)
  undocumented: remotefilelog.pullprefetch (str)
  undocumented: remotefilelog.reponame (str)
  undocumented: remotefilelog.server (bool)
  undocumented: remotefilelog.servercachepath (str)
  undocumented: remotefilelog.serverexpiration (int) [30]
  undocumented: remotefilelog.shallowtrees (bool)
  undocumented: remotefilelog.validatecache (str) ['on']
  undocumented: remotefilelog.validatecachelog (str)
  undocumented: simplecache.cachedir (str)
  undocumented: simplecache.caches (list) [['local']]
  undocumented: simplecache.evictionpercent (int) [50]
  undocumented: simplecache.host (str) ['localhost']
  undocumented: simplecache.maxcachesize (int) [2000]
  undocumented: simplecache.port (str) [11101]
  undocumented: simplecache.version (str) ['1']
  undocumented: smartlog.ignorebookmarks (str) ['!']
  undocumented: ssl.timeout (int) [5]
  undocumented: treemanifest.autocreatetrees (bool)
  undocumented: treemanifest.verifyautocreate (bool)
  undocumented: ui.editor.chunkselector (str)
