#chg-compatible

  $ cat >> foo.py << EOF
  > from edenscm.mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('foo', [], norepo=True)
  > def foo(ui):
  >     ui.write('This is the foo command\n')
  > EOF

  $ setconfig "extensions.foo=python-base64:`python -c 'import base64; print(base64.b64encode(open(\"foo.py\", "rb").read()).decode("utf-8").replace(\"\\n\",\"\"))'`"

  $ hg foo
  This is the foo command
