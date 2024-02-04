#debugruntest-compatible
#require ipython

  $ eagerepo
  $ cat >> foo.py << EOF
  > def f1(x): return x + 1
  > ui.write('OUT: %r\n' % [f1(i) for i in [1]])
  > EOF

  $ hg debugshell < foo.py
  OUT: [2]

  $ hg debugshell foo.py
  OUT: [2]

  $ hg debugshell -c 'def f2(x):
  >   return x+1
  > ui.write("OUT: %r\n" % [f2(i) for i in [1]])
  > '
  OUT: [2]

  $ hg debugshell --config ui.interactive=true << 'EOF' | dos2unix
  > def f3(x): return x + 1
  > ui.write('OUT: %r\n' % [f3(i) for i in [1]])
  > ui.flush()
  > exit()
  > EOF
  ...
  In [1]: 
  In [2]: OUT: [2]
  ...
