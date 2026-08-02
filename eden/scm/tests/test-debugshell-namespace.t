#require ipython no-eden

  $ eagerepo
  $ cat >> foo.py << EOF
  > def f1(x): return x + 1
  > ui.write('OUT: %r\n' % [f1(i) for i in [1]])
  > EOF

  $ sl debugshell < foo.py
  OUT: [2]

  $ sl debugshell foo.py
  OUT: [2]

  $ sl debugshell -c 'def f2(x):
  >   return x+1
  > ui.write("OUT: %r\n" % [f2(i) for i in [1]])
  > '
  OUT: [2]

  $ cat > check_magics.py << 'EOF'
  > from IPython.core.interactiveshell import InteractiveShell
  > from sapling.ext import debugshell
  > shell = InteractiveShell.instance()
  > debugshell._configipython(ui, shell)
  > ui.write('sl magic registered: %s\n' % (shell.find_line_magic('sl') is not None))
  > ui.write('hg magic registered: %s\n' % (shell.find_line_magic('hg') is not None))
  > ui.write('sl version exit: %s\n' % shell.run_line_magic('sl', 'version --quiet'))
  > EOF
  $ sl debugshell check_magics.py
  sl magic registered: True
  hg magic registered: False
  Sapling * (glob)
  sl version exit: 0
