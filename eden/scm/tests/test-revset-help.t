#debugruntest-compatible

  $ configure modern

  $ cat >> checkdoc.py << 'EOF'
  > from edenscm import revset, help, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > special_names = {"id", "allprecursors", "precursors"}
  > @command('checkdoc')
  > def checkdoc(ui, repo):
  >     excluded = help._exclkeywords
  >     for name, func in revset.predicate._table.items():
  >         if name.startswith("_"):
  >             continue
  >         doc = func.__doc__
  >         if not doc:
  >             ui.write("%s: no doc\n" % name)
  >             continue
  >         funcname = func.__name__
  >         if funcname != name and name not in funcname and name not in special_names:
  >             ui.write("%s: function name (%s) differs\n" % (name, funcname))
  > EOF

This command should have empty output:

  $ newrepo
  $ hg --config extensions.checkdoc="$TESTTMP/checkdoc.py" checkdoc
