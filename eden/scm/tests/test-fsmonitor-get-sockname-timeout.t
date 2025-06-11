#debugruntest-incompatible
#require fsmonitor no-windows

  $ enable fsmonitor hgevents
  $ setconfig experimental.fsmonitor.transaction_notify=true

  $ newclientrepo
  $ cat > $TESTTMP/wait_forever.py <<EOS
  > #!/usr/bin/env python
  > import time
  > time.sleep(3600)
  > EOS
  $ chmod +x $TESTTMP/wait_forever.py

Don't wait forever for the "bad" watchman:
  $ WATCHMAN_SOCK= WATCHMAN_BINARY=$TESTTMP/wait_forever.py hg bookmark -r . foo --debug
  error sending watchman state-enter for hg.transaction: warning: Watchman unavailable: watchman get-sockname exited with code -9 (timed_out=True timeout=1.000000 stdout= stderr=)
