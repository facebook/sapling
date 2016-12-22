#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import shutil
import subprocess
import tempfile
import time

import eden.thrift
from fb303.ttypes import fb_status
from .find_executables import EDEN_CLI, EDEN_DAEMON


class EdenFS(object):
    '''Manages an instance of the eden fuse server.'''

    def __init__(self, eden_dir=None, system_config_dir=None, home_dir=None):
        if eden_dir is None:
            eden_dir = tempfile.mkdtemp(prefix='eden_test.')
        self._eden_dir = eden_dir

        self._process = None
        self._system_config_dir = system_config_dir
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
        if self._process is None or self._process.returncode is not None:
            return
        self.shutdown()

    def _wait_for_healthy(self, timeout):
        '''Wait for edenfs to start and report that it is health.

        Throws an error if it doesn't come up within the specified time.
        '''
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                with self.get_thrift_client() as client:
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
        try:
            completed_process = subprocess.run(cmd, stdout=subprocess.PIPE,
                                               stderr=subprocess.PIPE,
                                               check=True)
        except subprocess.CalledProcessError as ex:
            # Re-raise our own exception type so we can include the error
            # output.
            raise EdenCommandError(ex)
        return completed_process.stdout.decode('utf-8')

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
        if self._system_config_dir:
            cmd += ['--system-config-dir', self._system_config_dir]
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

        args = self._get_eden_args(
                'daemon',
                '--daemon-binary', EDEN_DAEMON,
                '--foreground',
            )
        # If the EDEN_GDB environment variable is set, run eden inside gdb
        # so a developer can debug crashes
        if os.environ.get('EDEN_GDB'):
            gdb_exit_handler = (
                'python gdb.events.exited.connect('
                'lambda event: '
                'gdb.execute("quit") if getattr(event, "exit_code", None) == 0 '
                'else False'
                ')'
            )
            gdb_args = [
                # Register a handler to exit gdb if the program finishes
                # successfully.
                # Start the program immediately when gdb starts
                '-ex', gdb_exit_handler,
                # Start the program immediately when gdb starts
                '-ex', 'run'
            ]
            args.append('--gdb')
            for arg in gdb_args:
                args.append('--gdb-arg=' + arg)

            # Starting up under GDB takes longer than normal.
            # Allow an extra 90 seconds (for some reason GDB can take a very
            # long time to load symbol information, particularly on dynamically
            # linked builds).
            timeout += 90

        # Turn up the VLOG level for the fuse server so that errors are logged
        # with an explanation when they bubble up to RequestData::catchErrors
        args.extend(['--', '--vmodule=RequestData=5'])

        self._process = subprocess.Popen(args)
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


class EdenCommandError(subprocess.CalledProcessError):
    def __init__(self, ex):
        super().__init__(ex.returncode, ex.cmd, output=ex.output,
                         stderr=ex.stderr)

    def __str__(self):
        return ("eden command '%s' returned non-zero exit status %d\n"
                "stderr=%s" % (self.cmd, self.returncode, self.stderr))


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
