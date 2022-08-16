#chg-compatible
#debugruntest-compatible

  $ cat >> foo.py << EOF
  > def f(x): return x + 1
  > ui.write('%r\n' % [f(i) for i in [1]])
  > EOF

  $ hg debugshell < foo.py
  [2]

  $ hg debugshell foo.py
  [2]

  $ hg debugshell -c 'def f(x):
  >   return x+1
  > ui.write("%r\n" % [f(i) for i in [1]])
  > '
  [2]
