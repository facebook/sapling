(the Python watchman client seems to have some issues under debugruntest on Windows)
#chg-compatible

#require fsmonitor

  $ configure modernclient
  $ setconfig checkout.use-rust=true

FIXME Test we emit watchman states across checkout:
  $ newclientrepo watchman-events
  $ enable hgevents
  $ drawdag <<'EOS'
  > A
  > EOS
This will automatically exit after 2 seconds of inactivity.
  $ hg debugwatchmansubscribe > ../watchman_out &
Give the subscription a chance to start.
  $ sleep 1
Code under test (this should send state events to watchman):
  $ SL_LOG=checkout_info=debug hg go -q $A
  DEBUG checkout_info: checkout_mode="rust"
Wait for debugwatchmansubscribe to exit.
  $ wait
  $ cat ../watchman_out
  {
    "clock": *, (glob)
    "files": [],
    "is_fresh_instance": true,
    "root": "$TESTTMP/watchman-events",
    "subscription": "test-subscription",
    "unilateral": true,
    "version": * (glob)
  }
  {
    "clock": *, (glob)
    "files": [
      "A"
    ],
    "is_fresh_instance": false,
    "root": "$TESTTMP/watchman-events",
    "since": *, (glob)
    "subscription": "test-subscription",
    "unilateral": true,
    "version": * (glob)
  }
