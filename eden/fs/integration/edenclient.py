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
from eden.thrift import create_thrift_client
import os
import shutil
import socket
import subprocess
import tempfile
import time

# gen_srcs in the TARGETS file populates this with the eden cli binary
EDEN_CLI = get_file_path('eden/fs/integration/eden-cli')
EDEN_DAEMON = get_file_path('eden/fs/integration/daemon')


class EdenClient(object):
    '''Manages an instance of the eden fuse server.'''

    def __init__(self):
        self._paths_to_clean = []
        self._config_dir = None
        self._mount_path = None

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
        self.shutdown_cmd()
        return 0

    def _create_dirs(self):
        '''Creates and sets _config_dir and _mount_path if not already set.'''
        if self._config_dir is None:
            self._config_dir = tempfile.mkdtemp(prefix='eden_test.config.')
            self._paths_to_clean.append(self._config_dir)

        if self._mount_path is None:
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

            time.sleep(0.1)
        raise Exception("edenfs didn't start within timeout of %s" % timeout)

    def get_thrift_client(self):
        return create_thrift_client(self._config_dir)

    def _get_eden_args(self, subcommand_with_args):
        '''Combines the specified subcommand args with the appropriate defaults.

        Args:
            subcommand_with_args: array of strings
        Returns:
            A list of arguments to run Eden that can be used with
            subprocess.Popen() or subprocess.check_call().
        '''
        return [
            EDEN_CLI,
            '--config-dir', self._config_dir,
        ] + subcommand_with_args

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
            self._get_eden_args([
                'init',
                '--repo', self._repo_path,
                '--mount', self._mount_path,
                self._client_name
            ])
        )

        self.daemon_cmd(self._config_dir, timeout)
        self.mount_cmd()

    def daemon_cmd(self, config_dir=None, timeout=10):
        if config_dir and self._config_dir is None:
            self._config_dir = config_dir
        else:
            self._create_dirs()

        subprocess.check_call(
            self._get_eden_args([
                'daemon',
                '--daemon-binary', EDEN_DAEMON,
                # Preserve the environment variables so that we can use them to
                # help track runaway processes from test runs.
                '-E',
            ])
        )
        self._wait_for_thrift(timeout)

    def shutdown_cmd(self):
        subprocess.check_call(
            self._get_eden_args([
                'shutdown',
            ])
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

        self.daemon_cmd()
        self.mount_cmd()

    def mount_cmd(self):
        '''Executes mount command'''

        subprocess.check_call(
            self._get_eden_args([
                'mount',
                self._client_name
            ])
        )

    def unmount_cmd(self):
        '''Executes unmount command'''
        subprocess.check_call(
            self._get_eden_args([
                'unmount',
                self._client_name
            ])
        )

    def in_proc_mounts(self):
        '''Gets all eden mounts found in /proc/mounts, and returns
        true if this eden instance exists in list.
        '''
        with open('/proc/mounts', 'r') as f:
            mounts = [line.split(' ')[1] for line in f.readlines()
                      if line.split(' ')[0] == 'edenfs']
        return self._mount_path in mounts

    def is_healthy(self):
        '''Executes `eden health` and returns True if it exited with code 0.'''
        if self._config_dir is not None:
            try:
                subprocess.check_call(
                    self._get_eden_args([
                        'health',
                    ])
                )
                return True
            except subprocess.CalledProcessError:
                return False
        else:
            return False

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
