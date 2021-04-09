#chg-compatible

  $ cat >> foo.py << EOF
  > from edenscm.mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('foo', [], norepo=True)
  > def foo(ui):
  >     ui.write('This is the foo command\n')
  > EOF

  $ setconfig "extensions.foo=python-base64:`hg debugsh -c 'import base64; ui.writebytes(base64.b64encode(open(\"foo.py\", \"rb\").read()).decode("utf-8").replace(\"\\n\",\"\").encode("utf-8"))'`"

  $ hg foo
  This is the foo command
