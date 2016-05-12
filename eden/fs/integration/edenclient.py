# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals
from libfb.parutil import get_file_path
import errno
import os
import shutil
import socket
import subprocess
import tempfile
import time

# gen_srcs in the TARGETS file populates this with the eden cli binary
EDEN_CLI = get_file_path('eden/fs/integration/eden-cli')


class EdenClient(object):
    '''Manages an instance of the eden fuse server.'''

    def __init__(self):
        self._proc = None
        self._paths_to_clean = []

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, tb):
        self.cleanup()

    def __del__(self):
        self.cleanup()

    def cleanup(self):
        '''Stop the instance and clean up its temporary directories'''
        rc = self.kill()
        self.cleanup_dirs()
        return rc

    def cleanup_dirs(self):
        '''Remove any temporary dirs we have created.'''
        for path in self._paths_to_clean:
            shutil.rmtree(path, ignore_errors=True)
        self._paths_to_clean = []

    def kill(self):
        '''Stops and unmounts this instance.'''

        if not self._proc:
            return None

        # Send SIGTERM first, and then SIGKILL if the child process
        # doesn't exit within 3 seconds.
        self._proc.terminate()
        for n in range(30):
            if self._proc.poll() is None:
                time.sleep(0.1)
                continue
            break
        else:
            self._proc.kill()
            self._proc.wait()

        rc = self._proc.returncode
        self._proc = None
        return rc

    def _create_dirs(self):
        if not self._config_dir:
            self._config_dir = tempfile.mkdtemp(prefix='eden_test.config.')
            self._paths_to_clean.append(self._config_dir)

        if not self._mount_path:
            self._mount_path = tempfile.mkdtemp(prefix='eden_test.mount.')
            self._paths_to_clean.append(self._mount_path)

    def _wait_for_thrift(self, timeout):
        '''Wait for thrift server to start.

        Throws an error if it doesn't come up within the specified time.
        '''
        sock_path = os.path.join(self._config_dir, 'socket')

        deadline = time.time() + timeout
        while time.time() < deadline:
            # Just check to see if we can connect to the thrift socket.
            s = socket.socket(socket.AF_UNIX)
            try:
                s.connect(sock_path)
                return
            except (OSError, socket.error) as ex:
                if ex.errno != errno.ENOENT:
                    raise
            if self._proc.poll() is not None:
                # The eden process died
                raise Exception("eden died during startup: exit code = %s" %
                                (self._proc.returncode,))

            time.sleep(0.1)
        raise Exception("edenfs didn't start within timeout of %s" % timeout)

    def init(
        self,
        repo_path,
        config_dir=None,
        mount_path=None,
        client_name='CLIENT',
        timeout=10
    ):
        '''Runs eden init and passes the specified parameters.

        Will raise an error if the mount doesn't complete in a timely
        fashion.

        If it returns successfully, the eden instance is guaranteed
        to be mounted.
        '''
        self._config_dir = config_dir
        self._repo_path = repo_path
        self._mount_path = mount_path
        self._client_name = client_name

        self._create_dirs()

        subprocess.check_call(
            [
                EDEN_CLI,
                '--config-dir', self._config_dir,
                'init',
                '--repo', self._repo_path,
                '--mount', self._mount_path,
                self._client_name
            ]
        )

        self._proc = subprocess.Popen(
            [
                EDEN_CLI,
                '--config-dir', self._config_dir,
                'daemon',
            ]
        )
        self._wait_for_thrift(timeout)

        subprocess.check_call(
            [
                EDEN_CLI,
                '--config-dir', self._config_dir,
                'mount',
                self._client_name
            ]
        )

    def mount(self, client_name='CLIENT', config_dir=None, timeout=10):
        '''Runs eden mount and passes the specified parameters.

        Will raise an error if the mount doesn't complete in a timely
        fashion.

        If it returns successfully, the eden instance is guaranteed
        to be mounted.
        '''

        # Ensure that we aren't already running something.
        self.kill()

        self._client_name = client_name
        if config_dir is not None:
            self._config_dir = config_dir

        self._create_dirs()

        self._proc = subprocess.Popen(
            [
                EDEN_CLI,
                '--config-dir', self._config_dir,
                'daemon',
            ]
        )
        self._wait_for_thrift(timeout)

        subprocess.check_call(
            [
                EDEN_CLI,
                '--config-dir', self._config_dir,
                'mount',
                self._client_name
            ]
        )

    @property
    def repo_path(self):
        return self._repo_path

    @property
    def mount_path(self):
        return self._mount_path

    @property
    def client_name(self):
        return self._client_name

    @property
    def config_dir(self):
        return self._config_dir
