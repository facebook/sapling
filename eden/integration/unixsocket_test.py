#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import select
import socket
import stat
import threading

from .lib import testcase


PAYLOAD = b"W00t\n"


@testcase.eden_repo_test
class UnixSocketTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def test_unixsock(self) -> None:
        """Create a UNIX domain socket in EdenFS and verify that a client
           can connect and write, and that the server side can accept
           and read data from it."""

        filename = os.path.join(self.mount, "example.sock")
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.setblocking(False)  # ensure that we don't block unexpectedly
        try:
            sock.bind(filename)
            st = os.lstat(filename)
            self.assertTrue(stat.S_ISSOCK(st.st_mode))
            sock.listen(1)

            class Client(threading.Thread):
                def run(self) -> None:
                    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    try:
                        s.setblocking(False)  # don't block here either
                        s.connect(filename)
                        s.send(PAYLOAD)
                    finally:
                        s.close()

            # spin up a thread to connect back to us and prove that
            # the socket actually functions
            client_thread = Client()
            client_thread.start()

            readable, _, _ = select.select([sock], [], [], 2)
            self.assertTrue(sock in readable, msg="sock should be ready for accept")
            conn, addr = sock.accept()
            data = conn.recv(len(PAYLOAD))
            self.assertEqual(PAYLOAD, data)
            conn.close()
        finally:
            sock.close()
