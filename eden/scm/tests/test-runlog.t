#chg-compatible

  $ enable progress
  $ setconfig extensions.rustprogresstest="$TESTDIR/runlogtest.py" runlog.enable=True runlog.progress_refresh=0

  $ waitforrunlog() {
  >   while ! cat .hg/runlog/*.json 2> /dev/null; do
  >     sleep 0.001
  >   done
  >   rm .hg/runlog/*
  >   # ignore windows race condition where runlogtest.py deletes file during touch
  >   touch $TESTTMP/go 2>/dev/null || true
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
  $ hg progresstest --waitfile=$TESTTMP/go --config progress.disable=True 2 &
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
  $ hg progresstest --waitfile=$TESTTMP/go --config progress.renderer=rust:simple 2 &
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "progress.renderer=rust:simple",
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
      "progress.renderer=rust:simple",
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
      "progress.renderer=rust:simple",
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
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "progress.renderer=rust:simple",
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

Make sure progress updates when runlog.progress_refresh set.
  $ rm $TESTTMP/go
  $ hg progresstest --waitfile=$TESTTMP/go --config runlog.progress_refresh=0.001 1 &
  $ waitforrunlog
  {
    "id": ".*", (re)
    "command": [
      "progresstest",
      "--waitfile=$TESTTMP/go",
      "--config",
      "runlog.progress_refresh=0.001",
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
      "runlog.progress_refresh=0.001",
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
  $ setconfig runlog.cleanup_chance=0 runlog.cleanup_threshold=0
  $ rm -f .hg/runlog/*
  $ hg root > /dev/null
  $ hg root > /dev/null
  $ ls .hg/runlog/* | wc -l | sed -e 's/ //g'
  4

Test we always clean up with chance=1
  $ setconfig runlog.cleanup_chance=1
  $ hg root > /dev/null
  $ ls .hg/runlog/* | wc -l | sed -e 's/ //g'
  2

Test runlog CLI command

Show completed entries (i.e. exited "root" entry)
  $ setconfig runlog.cleanup_chance=0
  $ rm -f .hg/runlog/*
  $ hg root > /dev/null
  $ hg debugrunlog --ended
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
  $ hg debugrunlog
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
  $ rm -rf .hg/runlog
  $ touch .hg/runlog
  $ hg root
  Error creating runlogger: * (glob)
  $TESTTMP/repo
