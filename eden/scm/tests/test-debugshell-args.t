
#require no-eden


  $ eagerepo
  $ cat >> foo.py << EOF
  > ui.write('argv = %r\n' % (sys.argv,))
  > EOF

  $ hg debugshell foo.py 1 2 3
  argv = ('foo.py', '1', '2', '3')
  $ hg debugshell -c "$(cat foo.py)" 1 2 3
  argv = ('1', '2', '3')
  $ hg debugshell < foo.py
  argv = ()

Wtih crash traceback:

  $ hg debugshell -c 'raise RuntimeError("x")'
  ...
    File "<string>", line 1, in <module>
  RuntimeError: x
  [1]

  $ cat > a.py << EOF
  > def f():
  >     raise RuntimeError('x')
  > f()
  > EOF
  $ hg debugshell a.py
  ...
    File "<string>", line 3, in <module>
    File "<string>", line 2, in f
  RuntimeError: x
  [1]
