#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import asyncio
import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from eden.fs.service.eden.thrift_types import CheckoutMode, CheckOutRevisionParams
from eden.integration.lib import testcase
from thrift.python.exceptions import ApplicationError, TransportError


@testcase.eden_test(run_io_uring=True)
class HttpCheckoutErrorRaceTest(testcase.EdenTestCase):
    def edenfs_extra_config(self) -> dict[str, list[str]]:
        config = super().edenfs_extra_config() or {}
        config["treecache"] = ["enable-in-memory-tree-caching = false"]
        return config

    async def _exercise_checkout_errors(self, pid: int) -> bool:
        """Run one dry-run checkout whose old and new tree loads both fail.

        Returns True when the daemon died instead of reporting the errors.
        """
        async with self.eden.get_async_thrift_client(timeout=15) as client:
            try:
                await asyncio.wait_for(
                    client.checkOutRevision(
                        mountPoint=os.fsencode(self.mount),
                        snapshotHash=b"/next/",
                        checkoutMode=CheckoutMode.DRY_RUN,
                        params=CheckOutRevisionParams(),
                    ),
                    15,
                )
            except ApplicationError as error:
                self.assertIn("json parse error", str(error))
            except TransportError:
                self.assertNotEqual(0, self.eden.wait_for_exit(timeout=5))
                self.assertFalse(Path(f"/proc/{pid}").exists())
                return True
            else:
                self.fail("expected paired tree fetch errors")
            self.assertEqual(pid, (await client.getDaemonInfo()).pid)
            return False

    async def test_parallel_checkout_errors(self) -> None:
        if sys.platform != "linux":
            self.skipTest("Linux HTTP backing store")
        fail = threading.Event()
        lock = threading.Lock()
        paired: dict[str, tuple[set[bool], threading.Event]] = {}
        counts: dict[tuple[str, bool], int] = {}
        names = [f"d{i:02d}/" for i in range(16)]

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                if self.path in ("/", "/next/"):
                    body = json.dumps(
                        {"entries": {name: {"size": 0} for name in names}}
                    ).encode()
                elif fail.is_set():
                    name = self.path.rstrip("/").split("/")[-1]
                    side = self.path.startswith("/next/")
                    with lock:
                        seen, ready = paired.setdefault(
                            name, (set(), threading.Event())
                        )
                        seen.add(side)
                        counts[(name, side)] = counts.get((name, side), 0) + 1
                        if len(seen) == 2:
                            ready.set()
                    ready.wait(5)
                    body = b"invalid-json"
                else:
                    body = b'{"entries":{}}'
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
            worker = threading.Thread(target=server.serve_forever, daemon=True)
            worker.start()
            try:
                self.eden.run_cmd(
                    "clone",
                    f"127.0.0.1:{server.server_port}",
                    self.mount,
                    "--backing-store",
                    "http",
                )
                for name in names:
                    Path(self.mount, name, "local").write_text("preserved")
                pid = self.eden.get_pid_via_thrift()
                fail.set()
                daemon_died = False
                for _ in range(16):
                    with lock:
                        paired.clear()
                    daemon_died = await self._exercise_checkout_errors(pid)
                    if daemon_died:
                        break
                # The paired tree errors are appended to the same unsynchronized
                # vector from two executor threads. ThreadSanitizer reports the
                # race and halts the daemon; optimized builds survive it, so the
                # race is only observable here under TSan.
                # TODO(lean-check): wrong behaviour; flipped in the next commit
                self.assertEqual(
                    os.environ.get("EDEN_TEST_TSAN") == "1",
                    daemon_died,
                    "daemon died collecting concurrent checkout errors",
                )
                if daemon_died:
                    return
                with lock:
                    self.assertEqual(32, len(counts))
                    self.assertTrue(all(n >= 16 for n in counts.values()), counts)
                fail.clear()
                for name in names:
                    self.assertEqual(
                        "preserved", Path(self.mount, name, "local").read_text()
                    )
                self.eden.shutdown()
            finally:
                fail.clear()
                with lock:
                    for _, ready in paired.values():
                        ready.set()
                server.shutdown()
                worker.join(timeout=5)
