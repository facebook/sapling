#chg-compatible

  $ enable progress
  $ setconfig extensions.rustprogresstest="$TESTDIR/runlogtest.py" runlog.enable=True

  $ waitforrunlog() {
  >   while ! cat .hg/runlog/* 2> /dev/null; do
  >     sleep 0.001
  >   done
  >   rm .hg/runlog/*
  >   touch $TESTTMP/go
  > }

  $ hg init repo && cd repo

Check basic command start/end.
  $ hg basiccommandtest --waitfile=$TESTTMP/go 123 &

  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "basiccommandtest",
      "--waitfile=$TESTTMP/go",
      "123"
    ],
    "pid": \d+, (re)
    "start_time": ".*", (re)
    "end_time": null,
    "exit_code": null
  } (no-eol)

  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "basiccommandtest",
      "--waitfile=$TESTTMP/go",
      "123"
    ],
    "pid": \d+, (re)
    "start_time": ".*", (re)
    "end_time": ".*", (re)
    "exit_code": 123
  } (no-eol)
