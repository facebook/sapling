#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import errno
import os
import shutil
import subprocess
import sys
import tempfile
import time

import eden.thrift
from fb303.ttypes import fb_status


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


class EdenFS(object):
    '''Manages an instance of the eden fuse server.'''

    def __init__(self, eden_dir=None, home_dir=None):
        if eden_dir is None:
            eden_dir = tempfile.mkdtemp(prefix='eden_test.')
        self._eden_dir = eden_dir

        self._process = None
        self._home_dir = home_dir

    @property
    def eden_dir(self):
        return self._eden_dir

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
        shutil.rmtree(self._eden_dir, ignore_errors=True)

    def kill(self):
        '''Stops and unmounts this instance.'''
        if self._process is None:
            return
        self.shutdown()

    def _wait_for_healthy(self, timeout):
        '''Wait for edenfs to start and report that it is health.

        Throws an error if it doesn't come up within the specified time.
        '''
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                client = self.get_thrift_client()
                if client.getStatus() == fb_status.ALIVE:
                    return
            except eden.thrift.EdenNotRunningError as ex:
                pass

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
        return eden.thrift.create_thrift_client(self._eden_dir)

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
        cmd = [EDEN_CLI, '--config-dir', self._eden_dir]
        if self._home_dir:
            cmd += ['--home-dir', self._home_dir]
        cmd.append(command)
        cmd.extend(args)
        return cmd

    def start(self, timeout=10):
        '''
        Run "eden daemon" to start the eden daemon.
        '''
        if self._process is not None:
            raise Exception('cannot start an already-running eden client')

        self._process = subprocess.Popen(
            self._get_eden_args(
                'daemon',
                '--daemon-binary', EDEN_DAEMON,
                '--foreground',
            )
        )
        self._wait_for_healthy(timeout)

    def shutdown(self):
        '''
        Run "eden shutdown" to stop the eden daemon.
        '''
        self.run_cmd('shutdown')
        return_code = self._process.wait()
        self._process = None
        if return_code != 0:
            raise Exception('eden exited unsuccessfully with status {}'.format(
                return_code))

    def add_repository(self, name, repo_path):
        '''
        Run "eden repository" to define a repository configuration
        '''
        self.run_cmd('repository', name, repo_path)

    def repository_cmd(self):
        '''
        Executes "eden repository" to list the repositories,
        and returns the output as a string.
        '''
        return self.run_cmd('repository')

    def list_cmd(self):
        '''
        Executes "eden list" to list the client directories,
        and returns the output as a string.
        '''
        return self.run_cmd('list')

    def clone(self, repo, path):
        '''
        Run "eden clone"
        '''
        # TODO: "eden clone" should handle creating the directory.
        if not os.path.isdir(path):
            os.mkdir(path)

        self.run_cmd('clone', repo, path)

    def unmount(self, path):
        '''
        Run "eden unmount <path>"
        '''
        self.run_cmd('unmount', path)

    def in_proc_mounts(self, mount_path):
        '''Gets all eden mounts found in /proc/mounts, and returns
        true if this eden instance exists in list.
        '''
        with open('/proc/mounts', 'r') as f:
            mounts = [line.split(' ')[1] for line in f.readlines()
                      if line.split(' ')[0] == 'edenfs']
        return mount_path in mounts

    def is_healthy(self):
        '''Executes `eden health` and returns True if it exited with code 0.'''
        return_code = self.run_unchecked('health')
        return return_code == 0


def can_run_eden():
    '''
    Determine if we can run eden.

    This is used to determine if we should even attempt running the
    integration tests.
    '''
    global _can_run_eden
    if _can_run_eden is None:
        _can_run_eden = _compute_can_run_eden()

    return _can_run_eden


_can_run_eden = None


def _compute_can_run_eden():
    # FUSE must be available
    if not os.path.exists('/dev/fuse'):
        return False

    # We must be able to start eden as root.
    # The daemon must either be setuid root, or we must have sudo priviliges.
    # Typically for the tests the daemon process is not setuid root,
    # so check if we have are able to run things under sudo.
    return _can_run_sudo()


def _can_run_sudo():
    cmd = ['/usr/bin/sudo', '-E', '/bin/true']
    with open('/dev/null', 'r') as dev_null:
        # Close stdout, stderr, and stdin, and call setsid() to make
        # sure we are detached from any controlling terminal.  This makes
        # sure that sudo can't prompt for a password if it needs one.
        # sudo will only succeed if it can run with no user input.
        process = subprocess.Popen(cmd, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, stdin=dev_null,
                                   preexec_fn=os.setsid)
    process.communicate()
    return process.returncode == 0
