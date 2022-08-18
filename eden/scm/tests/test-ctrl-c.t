#require no-windows
#debugruntest-compatible
#inprocess-hg-incompatible

  $ cat > sleep.py << 'EOF'
  > import os
  > with open("pid", "w") as f:
  >     f.write("%s\n" % os.getpid())
  > b.sleep(100)
  > EOF

  $ ( hg dbsh sleep.py && echo exited; ) >out 2>err &

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

Should have "interrupted!" printed by dispatch.py:

  $ cat err
  interrupted!

Should not have "exited" printed by "echo exited" because non-zero exit code:

  $ cat out

Clean up background jobs:

  $ wait
