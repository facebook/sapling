#require fsmonitor no-windows

TODO: something with "while ! grep"
#debugruntest-incompatible

  $ setconfig experimental.fsmonitor.transaction_notify=true

  $ newclientrepo
  $ enable hgevents
This will automatically exit after 2 seconds of inactivity.
  $ hg debugwatchmansubscribe > ../watchman_out &

Give the subscription a chance to start.
  $ while ! grep "clock" ../watchman_out > /dev/null; do sleep 0.1; done

Code under test (this doesn't need to send hg.transaction event):
  $ hg dbsh <<EOS
  > from time import sleep
  > with repo.wlock():
  >   sleep(1)
  > EOS

Wait for debugwatchmansubscribe to exit.
  $ wait

No need to send hg.transaction event without a working copy.
  $ cat ../watchman_out
  {
    "clock": *, (glob)
    "files": [],
    "is_fresh_instance": true,
    "root": "$TESTTMP/repo1",
    "subscription": "test-subscription",
    "unilateral": true,
    "version": * (glob)
  }
