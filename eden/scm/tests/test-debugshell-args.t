#debugruntest-compatible

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
