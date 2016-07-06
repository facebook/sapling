#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import errno
from eden.thrift import create_thrift_client
import os
import shutil
import socket
import subprocess
import sys
import time


def _find_executables():
    '''Find the eden CLI and edenfs daemon relative to the unit test binary.'''
    test_binary = os.path.abspath(sys.argv[0])
    edenfs_dir = os.path.dirname(os.path.dirname(test_binary))
    cli = os.path.join(edenfs_dir, 'cli', 'cli.par')
    # The EDENFS_SUFFIX will be set to indicate if we should test with a
    # particular variant of the edenfs daemon
    suffix = os.environ.get('EDENFS_SUFFIX', '')
    edenfs = os.path.join(edenfs_dir, 'service', 'edenfs' + suffix)

    if not os.access(cli, os.X_OK):
        msg = 'unable to find eden CLI for integration testing: {!r}'.format(
            cli)
        raise Exception(msg)

    if not os.access(edenfs, os.X_OK):
        msg = 'unable to find eden daemon for integration testing: {!r}'.format(
            edenfs)
        raise Exception(msg)

    return cli, edenfs


EDEN_CLI, EDEN_DAEMON = _find_executables()


class EdenClient(object):
    '''Manages an instance of the eden fuse server.'''

    def __init__(self, eden_test_case):
        self._test_case = eden_test_case
        self.name = self._test_case.register_eden_client(self)
        self._config_dir = os.path.join(self._test_case.tmp_dir,
                                        self.name + '.config')
        self._mount_path = os.path.join(self._test_case.tmp_dir,
                                        self.name + '.mount')
        self._process = None
        self._home_dir = self._test_case.new_tmp_dir()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, tb):
        self.cleanup()

    def cleanup(self):
        '''Stop the instance and clean up its temporary directories'''
        self.kill()
        self.cleanup_dirs()

    def cleanup_dirs(self):
        '''Remove any temporary dirs we have created.'''
        shutil.rmtree(self._config_dir, ignore_errors=True)
        shutil.rmtree(self._home_dir, ignore_errors=True)
        shutil.rmtree(self._mount_path, ignore_errors=True)

    def kill(self):
        '''Stops and unmounts this instance.'''
        if self._process is None:
            return
        self.shutdown_cmd()

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

            status = self._process.poll()
            if status is not None:
                if status < 0:
                    msg = 'terminated with signal {}'.format(-status)
                else:
                    msg = 'exit status {}'.format(status)
                raise Exception('edenfs exited before becoming healthy: ' +
                                msg)

            time.sleep(0.1)
        raise Exception("edenfs didn't start within timeout of %s" % timeout)

    def get_thrift_client(self):
        return create_thrift_client(self._config_dir)

    def run_cmd(self, command, *args):
        '''
        Run the specified eden command.

        Args: The eden command name and any arguments to pass to it.
        Usage example: run_cmd('mount', 'my_eden_client')
        Throws a subprocess.CalledProcessError if eden exits unsuccessfully.
        '''
        cmd = self._get_eden_args(command, *args)
        return subprocess.check_output(cmd).decode('utf-8')

    def run_unchecked(self, command, *args):
        '''
        Run the specified eden command.

        Args: The eden command name and any arguments to pass to it.
        Usage example: run_cmd('mount', 'my_eden_client')
        Returns the process return code.
        '''
        cmd = self._get_eden_args(command, *args)
        return subprocess.call(cmd)

    def _get_eden_args(self, command, *args):
        '''Combines the specified eden command args with the appropriate
        defaults.

        Args:
            command: the eden command
            *args: extra string arguments to the command
        Returns:
            A list of arguments to run Eden that can be used with
            subprocess.Popen() or subprocess.check_call().
        '''
        return [
            EDEN_CLI,
            '--config-dir', self._config_dir,
            '--home-dir', self._home_dir,
            command,
        ] + list(args)

    def init(self, repo_path, client_name='CLIENT', timeout=10):
        '''Runs eden init and passes the specified parameters.

        Will raise an error if the mount doesn't complete in a timely
        fashion.

        If it returns successfully, the eden instance is guaranteed
        to be mounted.
        '''
        self._repo_path = repo_path
        self._client_name = client_name

        self.run_cmd('repository',
                     self._client_name,
                     self._repo_path)

        self.daemon_cmd(timeout)
        self.clone_cmd()

    def daemon_cmd(self, timeout=10):
        if self._process is not None:
            raise Exception('cannot start an already-running eden client')

        self._process = subprocess.Popen(
            self._get_eden_args(
                'daemon',
                '--daemon-binary', EDEN_DAEMON,
                '--foreground',
            )
        )
        self._wait_for_thrift(timeout)

    def shutdown_cmd(self):
        self.run_cmd('shutdown')
        return_code = self._process.wait()
        self._process = None
        if return_code != 0:
            raise Exception('eden exited unsuccessfully with status {}'.format(
                return_code))

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

    def repository_cmd(self):
        '''Executes repository command'''

        return self.run_cmd('repository')

    def clone_cmd(self):
        '''Executes clone command'''

        if not os.path.isdir(self.mount_path):
            os.mkdir(self._mount_path)

        self.run_cmd('clone', self._client_name, self._mount_path)

    def list_cmd(self):
        '''Executes list command'''

        return self.run_cmd('list')

    def mount_cmd(self):
        '''Executes mount command'''

        self.run_cmd('mount', self._client_name)

    def unmount_cmd(self):
        '''Executes unmount command'''
        self.run_cmd('unmount', self._mount_path)

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
        if self._config_dir is None:
            return False

        return_code = self.run_unchecked('health')
        return return_code == 0

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
