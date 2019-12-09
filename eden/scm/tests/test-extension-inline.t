#chg-compatible

  $ cat >> foo.py << EOF
  > from edenscm.mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('foo', [], norepo=True)
  > def foo(ui):
  >     ui.write('This is the foo command\n')
  > EOF

  $ setconfig "extensions.foo=python-base64:`python -c 'import base64; print(base64.encodestring(open(\"foo.py\").read()).replace(\"\\n\",\"\"))'`"

  $ hg foo
  This is the foo command
