#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"

Sanity check check-config.py

  $ cat > testfile.py << EOF
  > # Good
  > foo = ui.config('ui', 'username')
  > # Missing
  > foo = ui.config('ui', 'doesnotexist')
  > # Missing different type
  > foo = ui.configint('ui', 'missingint')
  > # Missing with default value
  > foo = ui.configbool('ui', 'missingbool1', default=True)
  > foo = ui.configbool('ui', 'missingbool2', False)
  > # Inconsistent values for defaults.
  > foo = ui.configint('ui', 'intdefault', default=1)
  > foo = ui.configint('ui', 'intdefault', default=42)
  > # Can suppress inconsistent value error
  > foo = ui.configint('ui', 'intdefault2', default=1)
  > # inconsistent config: ui.intdefault2
  > foo = ui.configint('ui', 'intdefault2', default=42)
  > EOF

  $ cat > files << EOF
  > mercurial/help/config.txt
  > $TESTTMP/testfile.py
  > EOF

  $ cd "$TESTDIR"/..

  $ $PYTHON contrib/check-config.py < $TESTTMP/files
  foo = ui.configint('ui', 'intdefault', default=42)
  conflict on ui.intdefault: ('int', '42') != ('int', '1')
  at $TESTTMP/testfile.py:12:
  undocumented: ui.doesnotexist (str)
  undocumented: ui.intdefault (int) [42]
  undocumented: ui.intdefault2 (int) [42]
  undocumented: ui.missingbool1 (bool) [True]
  undocumented: ui.missingbool2 (bool)
  undocumented: ui.missingint (int)

New errors are not allowed. Warnings are strongly discouraged.

  $ testrepohg files "set:(**.py or **.txt) - tests/**" | sed 's|\\|/|g' |
  >   $PYTHON contrib/check-config.py
      if ui.configbool('remotefilelog', 'fastdatapack', True):
  conflict on remotefilelog.fastdatapack: ('bool', 'True') != ('bool', '')
  at fb-hgext/tests/perftest.py:342:
              usecdatapack=ui.configbool('remotefilelog', 'fastdatapack'))
  conflict on remotefilelog.fastdatapack: ('bool', '') != ('bool', 'True')
  at fb-hgext/tests/treemanifest_correctness.py:36:
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
  undocumented: format.usehgsql (bool)
  undocumented: git.public (list)
  undocumented: grep.command (str)
  undocumented: hggit.usephases (bool)
  undocumented: hgsql.bypass (bool)
  undocumented: hgsql.database (str)
  undocumented: hgsql.enabled (bool)
  undocumented: hgsql.host (str)
  undocumented: hgsql.locktimeout (str)
  undocumented: hgsql.password (str)
  undocumented: hgsql.port (int)
  undocumented: hgsql.profileoutput (str)
  undocumented: hgsql.profiler (str)
  undocumented: hgsql.reponame (str)
  undocumented: hgsql.user (str)
  undocumented: hgsql.verifybatchsize (int)
  undocumented: hgsql.waittimeout (str)
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
  undocumented: remotenames.alias.default (bool)
  undocumented: remotenames.allownonfastforward (bool)
  undocumented: remotenames.calculatedistance (bool)
  undocumented: remotenames.disallowedbookmarks (list)
  undocumented: remotenames.disallowedhint (str)
  undocumented: remotenames.disallowedto (str)
  undocumented: remotenames.fastheaddiscovery (bool)
  undocumented: remotenames.forcecompat (bool)
  undocumented: remotenames.forceto (bool)
  undocumented: remotenames.pushanonheads (bool)
  undocumented: remotenames.pushrev (str)
  undocumented: remotenames.resolvenodes (bool)
  undocumented: remotenames.selectivepull (bool)
  undocumented: remotenames.selectivepulldefault (list)
  undocumented: remotenames.suppressbranches (bool)
  undocumented: remotenames.syncbookmarks (bool)
  undocumented: remotenames.tracking (bool)
  undocumented: remotenames.transitionmessage (str)
  undocumented: remotenames.upstream (list)
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
