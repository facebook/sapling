---
sidebar_position: 47
---

## web | isl
<!--
  @generated SignedSource<<ad7c8912d66412a4d2be050e92e408a2>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**launch Sapling Web GUI on localhost**

Sapling Web is a collection of web-based tools including Interactive Smartlog,
which is a GUI that facilitates source control operations such as creating,
reordering, or rebasing commits.
Running this command launches a web server that makes Sapling Web and
Interactive Smartlog available in a local web browser.

Examples:

Launch Sapling Web locally on port 8081:

```
$ sl web --port 8081
Listening on http://localhost:8081/?token=bbe168b7b4af1614dd5b9ddc48e7d30e&cwd=%2Fhome%2Falice%2Fsapling
Server logs will be written to /dev/shm/tmp/isl-server-logrkrmxp/isl-server.log
```

Using the `--json` option to get the current status of Sapling Web:

```
$ sl web --port 8081 --json | jq
{
    "url": "http://localhost:8081/?token=bbe168b7b4af1614dd5b9ddc48e7d30e&cwd=%2Fhome%2Falice%2Fsapling",
    "port": 8081,
    "token": "bbe168b7b4af1614dd5b9ddc48e7d30e",
    "pid": 1521158,
    "wasServerReused": true,
    "logFileLocation": "/dev/shm/tmp/isl-server-logrkrmxp/isl-server.log",
    "cwd": "/home/alice/sapling"
}
```

Using the `--kill` option to shut down the server:

```
$ sl web --port 8081 --kill
killed ISL server process 1521158
```

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-p`| `--port`| `3011`| port for Sapling Web|
| | `--json`| `false`| output machine-readable JSON|
| | `--open`| `true`| open Sapling Web in a local browser|
| `-f`| `--foreground`| `false`| keep the server process in the foreground|
| | `--kill`| `false`| kill any running server process, but do not start a new server|
| | `--force`| `false`| kill any running server process, then start a new server|
