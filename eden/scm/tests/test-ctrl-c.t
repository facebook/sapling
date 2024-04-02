#require no-windows no-eden
#debugruntest-compatible
#inprocess-hg-incompatible

  $ eagerepo
  $ cat > sleep.py << 'EOF'
  > import os
  > with open("pid", "w") as f:
  >     f.write("%s\n" % os.getpid())
  > for name in ["f1", "f2", "f3", "f4"]:
  >     with open(name, "w") as f:
  >         pass
  > a1 = b.atexit.AtExit.rmtree("f1")
  > a2 = b.atexit.AtExit.rmtree("f2")
  > a3 = b.atexit.AtExit.rmtree("f3")
  > a4 = b.atexit.AtExit.rmtree("f4")
  > with a4:
  >     with a3:
  >         a1.cancel()
  >     b.sleep(3000)
  > print("Should not be printed")
  > EOF

  $ ( hg dbsh sleep.py; echo $?; ) >out 2>err &

  $ cat > interrupt.py << 'EOF'
  > import time, os, signal, sys
  > for tick in range(1000):
  >     if not os.path.exists("pid") or not open("pid").read():
  >         time.sleep(1)
  > pid = int(open("pid").read())
  > os.kill(pid, signal.SIGINT)
  > try:
  >     for _ in range(30):
  >         os.kill(pid, 0)  # raise if pid no loner exists
  >         time.sleep(1)
  > except Exception:
  >     pass
  > EOF

  $ hg debugpython -- interrupt.py

Should exit with 130 showing that the Rust ctrlc handler is used:
(Python exits with 255)

  $ cat out
  130

f1 should exist (AtExit cancelled)
f2 should be removed by AtExit
f3 should exist (AtExit exited with context)
f4 should be removed (AtExit in with context):

  $ echo f*
  f1 f3

Nothing in stderr:

  $ cat err

Clean up background jobs:

  $ wait
