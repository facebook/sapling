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

  $ hg help config > $TESTTMP/config.txt

  $ cat > files << EOF
  > $TESTTMP/config.txt
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
  $ testrepohg files . | egrep -v '^tests/' | egrep '\.(py|txt)$' | sed 's|\\|/|g' |
  >   $PYTHON contrib/check-config.py
  undocumented: clone.requestfullclone (bool)
  undocumented: extensions.treemanifest (str)
  undocumented: fastlog.enabled (bool)
  undocumented: fastmanifest.cachecutoffdays (int) [60]
  undocumented: fastmanifest.cacheonchange (bool)
  undocumented: fastmanifest.cacheonchangebackground (bool) [True]
  undocumented: fastmanifest.debugmetrics (bool)
  undocumented: fastmanifest.logfile (str)
  undocumented: fastmanifest.relevantremotenames (list) [["master"]]
  undocumented: fastmanifest.silent (bool)
  undocumented: fastmanifest.silentworker (bool) [True]
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
  undocumented: grep.biggrepcorpus (str)
  undocumented: grep.biggreptier (str) ["biggrep.master"]
  undocumented: grep.command (str)
  undocumented: hggit.disallowinitbare (bool)
  undocumented: hggit.indexedlognodemap (bool)
  undocumented: hggit.usephases (bool)
  undocumented: hgsql.bypass (bool)
  undocumented: hgsql.database (str)
  undocumented: hgsql.enabled (bool)
  undocumented: hgsql.engine (str)
  undocumented: hgsql.host (str)
  undocumented: hgsql.locktimeout (str)
  undocumented: hgsql.port (int)
  undocumented: hgsql.profileoutput (str)
  undocumented: hgsql.user (str)
  undocumented: hgsql.verbose (bool)
  undocumented: hgsql.verifybatchsize (int)
  undocumented: hgsubversion.externals (str) ["svnexternals"]
  undocumented: hgsubversion.failoninvalidreplayfile (bool)
  undocumented: hgsubversion.failonmissing (bool)
  undocumented: hgsubversion.filestoresize (int) [200]
  undocumented: hgsubversion.nativerevs (bool)
  undocumented: hgsubversion.password (str)
  undocumented: hgsubversion.password_stores (list)
  undocumented: hgsubversion.reposubdir (str)
  undocumented: hgsubversion.repouuid (str)
  undocumented: hgsubversion.revmapimpl (str)
  undocumented: hgsubversion.startrev (str)
  undocumented: hgsubversion.stupid (bool)
  undocumented: hgsubversion.tunnels (list)
  undocumented: hgsubversion.unsafeskip (list)
  undocumented: hgsubversion.usecommitlayout (bool)
  undocumented: hgsubversion.username (str)
  undocumented: infinitepush.bgssh (str)
  undocumented: infinitepush.bundle-stream (bool)
  undocumented: morestatus.show (bool)
  undocumented: nointerrupt.interactiveonly (bool) [True]
  undocumented: perftweaks.disablecasecheck (bool)
  undocumented: perftweaks.disabletags (bool)
  undocumented: phabricator.arcrc_host (str)
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
  undocumented: remotefilelog.includepattern (list)
  undocumented: remotefilelog.pullprefetch (str)
  undocumented: remotefilelog.reponame (str)
  undocumented: remotefilelog.server (bool)
  undocumented: remotefilelog.servercachepath (str)
  undocumented: remotefilelog.serverexpiration (int) [30]
  undocumented: remotefilelog.shallowtrees (bool)
  undocumented: remotefilelog.validatecache (str) ["on"]
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
  undocumented: remotenames.hoist (str)
  undocumented: remotenames.pushanonheads (bool)
  undocumented: remotenames.pushrev (str)
  undocumented: remotenames.resolvenodes (bool)
  undocumented: remotenames.selectivepull (bool)
  undocumented: remotenames.selectivepullaccessedbookmarks (bool)
  undocumented: remotenames.selectivepulldefault (list)
  undocumented: remotenames.syncbookmarks (bool)
  undocumented: remotenames.tracking (bool)
  undocumented: remotenames.transitionmessage (str)
  undocumented: remotenames.upstream (list)
  undocumented: server.requireexplicitfullclone (bool)
  undocumented: smartlog.ignorebookmarks (str) ["!"]
  undocumented: ssl.timeout (int) [5]
  undocumented: treemanifest.verifyautocreate (bool)
  undocumented: ui.editor.chunkselector (str)
