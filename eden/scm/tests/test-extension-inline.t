#chg-compatible
#debugruntest-compatible

  $ cat >> foo.py << EOF
  > from edenscm import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('foo', [], norepo=True)
  > def foo(ui):
  >     ui.write('This is the foo command\n')
  > EOF

  >>> import base64
  >>> with open('foo.py', 'rb') as inf, open('foo.txt', 'wb') as outf:
  ...   outf.write(base64.b64encode(inf.read())) and None

  $ setconfig "extensions.foo=python-base64:$(cat foo.txt)"

  $ hg foo
  This is the foo command
