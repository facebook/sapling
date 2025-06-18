
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
  Traceback (most recent call last):
  ...
    File "debugshell:script", line 1, in <module>
      raise RuntimeError("x")
  RuntimeError: x
  [1]

  $ cat > a.py << EOF
  > def f():
  >     raise RuntimeError('x')
  > f()
  > EOF
  $ hg debugshell a.py
  Traceback (most recent call last):
  ...
    File "a.py", line 3, in <module>
      f()
    File "a.py", line 2, in f
      raise RuntimeError('x')
  RuntimeError: x
  [1]
