#require bash no-eden

  $ eagerepo
  $ enable progress
  $ setconfig extensions.rustprogresstest="$TESTDIR/runlogtest.py" runlog.enable=True runlog.progress-refresh=0
  $ export LOG=runlog=warn

  $ waitforrunlog() {
  >   while ! cat .sl/runlog/*.json 2> /dev/null; do
  >     sleep 0.001
  >   done
  >   rm .sl/runlog/*
  >   # ignore windows race condition where runlogtest.py deletes file during touch
  >   touch $TESTTMP/go 2>/dev/null || true
  > }

  $ sl init repo && cd repo

Check basic command start/end.
  $ sl basiccommandtest --waitfile=$TESTTMP/go 123 &

  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "basiccommandtest",
      "--waitfile=$TESTTMP/go",
      "123"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": null,
    "exit_code": null,
    "progress": []
  } (no-eol)

  $ wait
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "basiccommandtest",
      "--waitfile=$TESTTMP/go",
      "123"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": ".*", (re)
    "exit_code": 123,
    "progress": []
  } (no-eol)

Make sure runlog works with progress disabled.
  $ sl progresstest --waitfile=$TESTTMP/go --config progress.disable=True 2 &
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "progress.disable=True",
      "2"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": null,
    "exit_code": null,
    "progress": []
  } (no-eol)
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "progress.disable=True",
      "2"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": null,
    "exit_code": null,
    "progress": [
      {
        "topic": "eating",
        "unit": "apples",
        "total": 2,
        "position": 1
      }
    ]
  } (no-eol)
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "progress.disable=True",
      "2"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": null,
    "exit_code": null,
    "progress": [
      {
        "topic": "eating",
        "unit": "apples",
        "total": 2,
        "position": 2
      }
    ]
  } (no-eol)
  $ wait
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "progress.disable=True",
      "2"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": ".*", (re)
    "exit_code": 0,
    "progress": []
  } (no-eol)

Make sure runlog works with rust renderer.
  $ rm $TESTTMP/go
  $ sl progresstest --waitfile=$TESTTMP/go --config progress.renderer=simple 2 &
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "progress.renderer=simple",
      "2"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": null,
    "exit_code": null,
    "progress": []
  } (no-eol)
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "progress.renderer=simple",
      "2"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": null,
    "exit_code": null,
    "progress": [
      {
        "topic": "eating",
        "unit": "apples",
        "total": 2,
        "position": 1
      }
    ]
  } (no-eol)
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "progress.renderer=simple",
      "2"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": null,
    "exit_code": null,
    "progress": [
      {
        "topic": "eating",
        "unit": "apples",
        "total": 2,
        "position": 2
      }
    ]
  } (no-eol)
  $ wait
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "progress.renderer=simple",
      "2"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": ".*", (re)
    "exit_code": 0,
    "progress": []
  } (no-eol)

Make sure progress updates when runlog.progress-refresh set.
  $ rm $TESTTMP/go
  $ sl progresstest --waitfile=$TESTTMP/go --config runlog.progress-refresh=0.001 1 &
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "runlog.progress-refresh=0.001",
      "1"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": null,
    "exit_code": null,
    "progress": []
  } (no-eol)
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "runlog.progress-refresh=0.001",
      "1"
    ],
    "pid": \d+, (re)
    "download_bytes": 0,
    "upload_bytes": 0,
    "start_time": ".*", (re)
    "end_time": null,
    "exit_code": null,
    "progress": [
      {
        "topic": "eating",
        "unit": "apples",
        "total": 1,
        "position": 1
      }
    ]
  } (no-eol)

Wait for background process to exit.
  $ waitforrunlog > /dev/null


Test we don't clean up entries with chance=0
  $ setconfig runlog.cleanup-chance=0 runlog.cleanup-threshold=0
  $ rm -f .sl/runlog/*
  $ sl root > /dev/null
  $ sl root > /dev/null
  $ ls .sl/runlog/* | grep -v watchfile | wc -l | sed -e 's/ //g'
  4

Test we always clean up with chance=1
  $ setconfig runlog.cleanup-chance=1
  $ sl root > /dev/null
  $ ls .sl/runlog/* | grep -v watchfile | wc -l | sed -e 's/ //g'
  2

Test runlog CLI command

Show completed entries (i.e. exited "root" entry)
  $ setconfig runlog.cleanup-chance=0
  $ rm -f .sl/runlog/*
  $ sl root > /dev/null
  $ sl debugrunlog --ended
  Entry {
      id: ".*", (re)
      command: [
          "root",
      ],
      pid: \d+, (re)
      download_bytes: 0,
      upload_bytes: 0,
      start_time: .*, (re)
      end_time: Some(
          .*, (re)
      ),
      exit_code: Some(
          0,
      ),
      progress: [],
  }

Show only running commands (i.e. "debugrunlog" command itself)
  $ sl debugrunlog
  Entry {
      id: ".*", (re)
      command: [
          "debugrunlog",
      ],
      pid: \d+, (re)
      download_bytes: 0,
      upload_bytes: 0,
      start_time: .*, (re)
      end_time: None,
      exit_code: None,
      progress: [],
  }

Test we don't bail out if we can't write runlog.
  $ rm -rf .sl/runlog
  $ touch .sl/runlog
  $ sl root
  Error creating runlogger: * (glob)
  $TESTTMP/repo
