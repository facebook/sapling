#chg-compatible
#debugruntest-compatible

Check pending changes to metalog is visible to hooks running in subprocesses.

The test works as follows:
- The `triggerpending` command starts a transaction and modifies metalog.
- The `hook.pretxnclose` hook gets executed with pending changes.
- The `showpending` command running in subprocess checks the pending changes in
metalog.

  $ configure modern

  $ cat > ext.py << 'EOF'
  > from edenscm.mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('showpending')
  > def showpending(ui, repo):
  >     ui.write("FOO: %r\n" % (repo.metalog()["FOO"] or b"").decode("utf-8"))
  > @command('triggerpending')
  > def triggerpending(ui, repo):
  >     with repo.lock(), repo.transaction("testpending") as tr:
  >         ml = repo.metalog()
  >         ml["FOO"] = b"BAR"
  > EOF

  $ setconfig 'hooks.pretxnclose=hg showpending' extensions.ext="$TESTTMP/ext.py"

  $ newrepo
  $ hg triggerpending
  FOO: 'BAR'
