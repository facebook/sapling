#require no-windows

  $ cat > sleep.py << 'EOF'
  > import os
  > with open("pid", "w") as f:
  >     f.write("%s\n" % os.getpid())
  > b.sleep(100)
  > EOF

  $ ( hg dbsh sleep.py && echo exited; ) >out 2>err &
  $ disown

  $ cat > interrupt.py << 'EOF'
  > import time, os, signal, sys
  > for tick in range(1000):
  >     if not os.path.exists("pid") or not open("pid").read():
  >         time.sleep(1)
  > pid = int(open("pid").read())
  > os.kill(pid, signal.SIGINT)
  > time.sleep(10)
  > EOF

  $ hg debugpython -- interrupt.py

Should have "interrupted!":

  $ cat err
  interrupted!
