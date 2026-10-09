# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

  $ . "$TEST_FIXTURES/library.sh"
  $ COMMIT_IDENTITY_SCHEME=3 GIT_LFS_INTERPRET_POINTERS=1 setup_common_config blob_files
  $ printf '%s\0' "$MONONOKE_GITIMPORT" "${CACHE_ARGS[@]}" "${COMMON_ARGS[@]}" --repo-id "$REPOID" --mononoke-config-path "$TESTTMP/mononoke-config" --tracing-test-format --persist-partial-mappings > "$TESTTMP/importer-argv"

Use a local GitHub LFS Batch server and dummy tokens. One importer handles multiple
imports, token rotation, a mapped rewind and bookmark repair before clean EOF.
Protocol failures and missing LFS contents must never produce completion responses.

  $ cat > "$TESTTMP/persistent-test.py" <<'PY'
  > import hashlib
  > import http.server
  > import json
  > import os
  > import pathlib
  > import re
  > import select
  > import signal
  > import sqlite3
  > import subprocess
  > import threading
  > import time
  > root = pathlib.Path(os.environ["TESTTMP"])
  > repo = root / "session-git"
  > repo.mkdir()
  > token_file = root / "session-token"
  > token_file.write_text("dummy-token-one")
  > state = {"token": "dummy-token-one", "objects": {}, "seen": [], "downloads": [], "unauthorized": 0}
  > class LfsHandler(http.server.BaseHTTPRequestHandler):
  >     def log_message(self, *args):
  >         pass
  >     def send(self, status, body):
  >         self.send_response(status)
  >         self.send_header("Content-Length", str(len(body)))
  >         self.end_headers()
  >         self.wfile.write(body)
  >     def do_POST(self):
  >         if self.headers.get("Authorization") != "Bearer " + state["token"]:
  >             state["unauthorized"] += 1
  >             return self.send(403, b"")
  >         state["seen"].append(state["token"])
  >         request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
  >         item = request["objects"][0]
  >         oid = item["oid"]
  >         response = {"oid": oid, "size": item["size"]}
  >         if oid in state["objects"]:
  >             response["actions"] = {"download": {"href": base_url + "/" + oid}}
  >         else:
  >             response["error"] = {"code": 404, "message": "test object absent"}
  >         self.send(200, json.dumps({"transfer": "basic", "objects": [response]}).encode())
  >     def do_GET(self):
  >         oid = self.path[1:]
  >         if oid not in state["objects"]:
  >             return self.send(404, b"")
  >         state["downloads"].append(oid)
  >         self.send(200, state["objects"][oid])
  > server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), LfsHandler)
  > base_url = "http://127.0.0.1:" + str(server.server_port)
  > threading.Thread(target=server.serve_forever, daemon=True).start()
  > git_env = dict(os.environ, GIT_AUTHOR_NAME="mononoke", GIT_AUTHOR_EMAIL="mononoke@mononoke",
  >                GIT_COMMITTER_NAME="mononoke", GIT_COMMITTER_EMAIL="mononoke@mononoke",
  >                GIT_AUTHOR_DATE="2000-01-01T00:00:00+0000", GIT_COMMITTER_DATE="2000-01-01T00:00:00+0000")
  > def git(*args):
  >     return subprocess.check_output(["git", "-C", str(repo), *args], env=git_env).decode().strip()
  > git("init", "-q", "-b", "master_bookmark")
  > def commit_payload(data, available=True):
  >     oid = hashlib.sha256(data).hexdigest()
  >     if available:
  >         state["objects"][oid] = data
  >     (repo / "large").write_text("version https://git-lfs.github.com/spec/v1\noid sha256:" + oid + "\nsize " + str(len(data)) + "\n")
  >     git("add", "large")
  >     git("commit", "-qm", "payload " + oid)
  >     return git("rev-parse", "HEAD"), oid
  > first, oid_one = commit_payload(b"first persistent LFS payload\n")
  > git("branch", "release")
  > (root / "oid-one").write_text(oid_one)
  > argv = [os.fsdecode(part) for part in (root / "importer-argv").read_bytes().split(b"\0") if part]
  > argv += ["--git-command-path", "git", str(repo), "--persistent", "--generate-bookmarks", "--suppress-ref-mapping",
  >          "--bypass-non-fast-forward", "--include-refs", "refs/heads/master_bookmark,refs/heads/release",
  >          "--github-lfs-url", base_url + "/objects/batch", "--github-lfs-token-file", str(token_file),
  >          "--github-lfs-no-https-proxy", "--lfs-import-max-attempts", "1", "--log-import-phases", "incremental"]
  > allowed = {"main_entered", "startup", "open_repo", "discover_commits", "import_contents", "resolve_refs",
  >            "open_managed_repo", "publication", "async_cleanup", "runtime_shutdown"}
  > sessions = []
  > class Session:
  >     def __init__(self):
  >         self.log = open(root / ("session-" + str(len(sessions)) + ".log"), "wb")
  >         self.proc = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log, start_new_session=True)
  >         self.buffer = b""
  >         self.count = 0
  >         sessions.append(self)
  >     def line(self):
  >         deadline = time.monotonic() + 120
  >         while b"\n" not in self.buffer:
  >             remaining = deadline - time.monotonic()
  >             assert remaining > 0 and select.select([self.proc.stdout], [], [], remaining)[0], "session response timeout"
  >             data = os.read(self.proc.stdout.fileno(), 4096)
  >             if not data:
  >                 assert not self.buffer
  >                 return None
  >             self.buffer += data
  >         line, self.buffer = self.buffer.split(b"\n", 1)
  >         return line
  >     def request(self, operation, refs, success=True):
  >         self.count += 1
  >         request = {"version": 1, "request_id": str(self.count), "repo_name": "repo", "operation": operation, "refs": refs}
  >         self.proc.stdin.write(json.dumps(request).encode() + b"\n")
  >         self.proc.stdin.flush()
  >         phases = []
  >         while True:
  >             line = self.line()
  >             if line is None:
  >                 assert not success
  >                 assert self.proc.wait(timeout=30) != 0
  >                 return phases
  >             if line.startswith(b"gitimport_session "):
  >                 assert success, "failed request emitted a completion"
  >                 response = json.loads(line[len(b"gitimport_session "):])
  >                 assert response == dict(request, status="ready" if operation == "warm" else "completed")
  >                 return phases
  >             match = re.fullmatch(rb"gitimport_phase phase=([a-z_]+) duration_ms=([0-9]+)", line)
  >             assert match and match[1].decode() in allowed, line
  >             phases.append(match[1].decode())
  >     def clean_eof(self):
  >         self.proc.stdin.close()
  >         tail = []
  >         while (line := self.line()) is not None:
  >             tail.append(line)
  >         assert self.proc.wait(timeout=30) == 0
  >         assert len(tail) == 1 and tail[0].startswith(b"gitimport_phase phase=runtime_shutdown ")
  >     def rejected_frame(self, frame):
  >         self.proc.stdin.write(frame)
  >         self.proc.stdin.flush()
  >         self.proc.stdin.close()
  >         while (line := self.line()) is not None:
  >             assert line.startswith(b"gitimport_phase ") and not line.startswith(b"gitimport_session ")
  >         assert self.proc.wait(timeout=30) != 0
  >     def failed_with(self, reason):
  >         stderr = pathlib.Path(self.log.name).read_text()
  >         assert "Persistent import session failed: " + reason in stderr, stderr
  >         assert "dummy-token" not in stderr, stderr
  >     def readers_drained(self):
  >         deadline = time.monotonic() + 10
  >         while True:
  >             children = subprocess.run(["ps", "--ppid", str(self.proc.pid), "-o", "comm="], capture_output=True).stdout
  >             if not children.strip():
  >                 return
  >             assert time.monotonic() < deadline, "per-request Git readers leaked: " + repr(children)
  >             time.sleep(0.05)
  > def sql(query, args=()):
  >     with sqlite3.connect(root / "monsql/sqlite_dbs") as db:
  >         return db.execute(query, args).fetchall()
  > def bookmark():
  >     return sql("SELECT lower(hex(g.git_sha1)) FROM bookmarks b JOIN bonsai_git_mapping g ON b.changeset_id=g.bcs_id AND b.repo_id=g.repo_id WHERE CAST(b.name AS TEXT)='heads/master_bookmark'")[0][0]
  > def snapshot(main):
  >     return {"refs/heads/master_bookmark": main, "refs/heads/release": first}
  > try:
  >     session = Session()
  >     phases = session.request("warm", {})
  >     assert phases == ["main_entered", "startup", "open_repo"], phases
  >     assert sql("SELECT count(*) FROM bookmarks")[0][0] == 0
  >     assert sql("SELECT count(*) FROM bonsai_git_mapping")[0][0] == 0
  >     assert not state["seen"] and not state["downloads"]
  >     phases = session.request("import", snapshot(first))
  >     assert phases == ["open_repo", "discover_commits", "import_contents", "resolve_refs", "open_managed_repo", "publication", "async_cleanup"], phases
  >     assert bookmark() == first and state["downloads"] == [oid_one]
  >     session.readers_drained()
  >     second, oid_two = commit_payload(b"second persistent LFS payload\n")
  >     (root / "oid-two").write_text(oid_two)
  >     token_file.write_text("dummy-token-two")
  >     state["token"] = "dummy-token-two"
  >     session.request("import", snapshot(second))
  >     assert bookmark() == second and state["seen"] == ["dummy-token-one", "dummy-token-two"]
  >     assert state["unauthorized"] == 0, "stale LFS token was reused across requests"
  >     assert state["downloads"] == [oid_one, oid_two]
  >     session.readers_drained()
  >     before = sql("SELECT count(*) FROM bookmarks_update_log")[0][0]
  >     session.request("import", snapshot(second))
  >     assert sql("SELECT count(*) FROM bookmarks_update_log")[0][0] == before
  >     git("update-ref", "refs/heads/master_bookmark", first)
  >     session.request("import", snapshot(first))
  >     assert bookmark() == first
  >     sql("UPDATE bookmarks SET changeset_id=(SELECT bcs_id FROM bonsai_git_mapping WHERE lower(hex(git_sha1))=?) WHERE CAST(name AS TEXT)='heads/master_bookmark'", (second,))
  >     session.request("import", snapshot(first))
  >     assert bookmark() == first
  >     session.readers_drained()
  >     session.clean_eof()
  >     print("warm, serial imports, fresh token, no-op, rewind, repair and EOF passed")
  >     before = sql("SELECT count(*) FROM bookmarks_update_log")[0][0]
  >     mismatch = Session()
  >     mismatch.request("import", snapshot(second), success=False)
  >     mismatch.failed_with("snapshot_mismatch")
  >     assert bookmark() == first
  >     assert sql("SELECT count(*) FROM bookmarks_update_log")[0][0] == before
  >     for frame, reason in [(b"{malformed}\n", "invalid_frame"), (b"x" * (128 * 1024 + 1), "frame_too_large"), (b'{"version":1', "incomplete_frame")]:
  >         bad = Session()
  >         bad.rejected_frame(frame)
  >         bad.failed_with(reason)
  >     assert sql("SELECT count(*) FROM bookmarks_update_log")[0][0] == before
  >     print("wrong SHA, malformed, oversized and partial frames rejected without publication")
  >     missing, missing_oid = commit_payload(b"absent persistent LFS payload\n", available=False)
  >     strict = Session()
  >     strict.request("warm", {})
  >     strict.request("import", snapshot(missing), success=False)
  >     strict.failed_with("import_contents_failed")
  >     assert bookmark() == first
  >     assert sql("SELECT count(*) FROM bookmarks_update_log")[0][0] == before
  >     assert sql("SELECT count(*) FROM bonsai_git_mapping WHERE lower(hex(git_sha1))=?", (missing,))[0][0] == 0
  >     assert missing_oid not in state["downloads"]
  >     print("missing LFS contents rejected before target mapping or bookmark publication")
  > finally:
  >     for session in sessions:
  >         try:
  >             os.killpg(session.proc.pid, signal.SIGKILL)
  >         except ProcessLookupError:
  >             pass
  >         session.proc.wait()
  >         session.log.close()
  >     server.shutdown()
  >     server.server_close()
  > PY
  $ python3 "$TESTTMP/persistent-test.py"
  warm, serial imports, fresh token, no-op, rewind, repair and EOF passed
  wrong SHA, malformed, oversized and partial frames rejected without publication
  missing LFS contents rejected before target mapping or bookmark publication

Verify actual Mononoke filestore bytes, independent of the mock's download log.

  $ mononoke_admin filestore -R repo fetch --content-sha256 "$(cat "$TESTTMP/oid-one")"
  first persistent LFS payload
  $ mononoke_admin filestore -R repo fetch --content-sha256 "$(cat "$TESTTMP/oid-two")"
  second persistent LFS payload
