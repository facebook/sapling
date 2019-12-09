#chg-compatible

  $ cat > a.py << EOF
  > import os
  > def uisetup(ui):
  >     # make the test slightly more interesting
  >     ui.fout = os.fdopen(ui.fout.fileno(), "wb", 1)
  >     @ui.atexit
  >     def printlines():
  >         ui.write("line1\n")
  >         ui.write("line2\n" * 10000)  # probably triggers EPIPE or SIGPIPE
  >         open("executed-here1", "w").close()
  > EOF

This should not trigger StdioError (IOError), or BrokenPipeError (OSError):

  $ hg --config extensions.a=a.py init foo1 | head -1
  line1

'executed-here1' should exist to indicate the execution flow:

  $ [ -f executed-here1 ]

Try again, using a pager:

  $ cat > b.py << EOF
  > import os
  > def uisetup(ui):
  >     ui.fout = os.fdopen(ui.fout.fileno(), "wb", 1)
  >     @ui.atexit
  >     def printlines():
  >         # This is hacky. But it makes sure pager is running.
  >         # Using --pager=always is not enough, because killpager is also
  >         # an atexit handler and gets executed before this one.
  >         ui.pager("internal-always-atexit")
  >         # Redo signal.signal(signal.SIGPIPE, signal.SIG_IGN) called by
  >         # _runexithandlers.
  >         import signal
  >         signal.signal(signal.SIGPIPE, signal.SIG_IGN)
  >         ui.write("line1\n")
  >         ui.write("line2\n" * 10000)  # probably triggers EPIPE or SIGPIPE
  >         open("executed-here2", "w").close()
  > EOF

This should not raise SignalInterrupt (KeyboardInterrupt):

  $ hg --config extensions.b=b.py --config 'pager.pager=head -1' init foo2
  line1

'executed-here2' should exist to indicate the execution flow:

  $ [ -f executed-here2 ]

