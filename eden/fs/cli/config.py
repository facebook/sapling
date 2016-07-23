#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import collections
import configparser
import errno
import fcntl
import hashlib
import json
import os
import shutil
import stat
import subprocess
import tempfile
import time

from . import util
import eden.thrift
from facebook.eden import EdenService
import facebook.eden.ttypes as eden_ttypes
from fb303.ttypes import fb_status
import thrift

# These are INI files that hold config data.
GLOBAL_CONFIG_DIR = '/etc/eden/config.d'
HOME_CONFIG = '.edenrc'

# These paths are relative to the user's client directory.
CLIENTS_DIR = 'clients'
STORAGE_DIR = 'storage'
ROCKS_DB_DIR = os.path.join(STORAGE_DIR, 'rocks-db')
CONFIG_JSON = 'config.json'

# These are files in a client directory.
LOCAL_CONFIG = 'edenrc'
SNAPSHOT = 'SNAPSHOT'

# In our test environment, when we need to run as root, we
# may need to launch via a helper script that is whitelisted
# by the local sudo configuration.
SUDO_HELPER = '/var/www/scripts/testinfra/run_eden.sh'


class EdenStartError(Exception):
    pass


class UsageError(Exception):
    pass


class Config:
    def __init__(self, config_dir, home_dir):
        self._config_dir = config_dir
        self._home_dir = home_dir

    def get_rc_files(self):
        rc_files = []
        if os.path.isdir(GLOBAL_CONFIG_DIR):
            rc_files = os.listdir(GLOBAL_CONFIG_DIR)
            rc_files = [os.path.join(GLOBAL_CONFIG_DIR, f) for f in rc_files]
        sorted(rc_files)
        config = os.path.join(self._home_dir, HOME_CONFIG)
        if os.path.isfile(config):
            rc_files.append(config)
        return rc_files

    def get_repository_list(self, parser=None):
        result = []
        if not parser:
            parser = configparser.ConfigParser()
            parser.read(self.get_rc_files())
        for section in parser.sections():
            header = section.split(' ')
            if len(header) == 2 and header[0] == 'repository':
                result.append(header[1])
        sorted(result)
        return result

    def get_repo_data(self, name):
        '''
        Returns a dictionary containing the metadata and the bind mounts of the
        repository specified by name and raises an exception if the repository
        data could not be found. The expected keys in the returned dictionary
        are: 'repo_type', 'repo_source', 'bind-mounts'.
        '''
        result = {}
        rc_files = self.get_rc_files()
        parser = configparser.ConfigParser()
        parser.read(rc_files)
        for section in parser.sections():
            header = section.split(' ')
            if len(header) == 2 and header[1] == name:
                if header[0] == 'repository':
                    result.update(parser[section])
                if header[0] == 'bindmounts':
                    result['bind-mounts'] = parser[section]
        if not result:
            raise Exception('repository %s does not exist.' % name)
        if 'bind-mounts' not in result:
            result['bind-mounts'] = {}
        return result

    def get_mount_paths(self):
        '''Return the paths of the set mount points stored in config.json'''
        return self._get_directory_map().keys()

    def get_all_client_config_info(self):
        info = {}
        for path in self.get_mount_paths():
            info[path] = self.get_client_info(path)

        return info

    def get_thrift_client(self):
        return eden.thrift.create_thrift_client(self._config_dir)

    def get_client_info(self, path):
        client_dir = self._get_client_dir_for_mount_point(path)
        repo_name = self._get_repo_name(client_dir)
        repo_data = self.get_repo_data(repo_name)

        snapshot_file = os.path.join(client_dir, SNAPSHOT)
        snapshot = open(snapshot_file).read().strip()

        return collections.OrderedDict([
            ['bind-mounts', repo_data['bind-mounts']],
            ['mount', path],
            ['snapshot', snapshot],
            ['client-dir', client_dir],
        ])

    def checkout(self, path, snapshot_id):
        '''Switch the active snapshot id for a given client'''
        with self.get_thrift_client() as client:
            client.checkOutRevision(path, snapshot_id)

    def add_repository(self, name, repo_type, source, with_buck=False):
        # Check if repository already exists
        config_ini = os.path.join(self._home_dir, HOME_CONFIG)

        with ConfigUpdater(config_ini) as config:
            if name in self.get_repository_list(config):
                raise UsageError('''\
repository %s already exists. You will need to edit the ~/.edenrc config file \
by hand to make changes to the repository or remove it.''' % name)

            # Create a directory for client to store repository metadata
            bind_mounts = {}
            if with_buck:
                bind_mount_name = 'buck-out'
                bind_mounts[bind_mount_name] = 'buck-out'

            # Add repository to INI file
            config['repository ' + name] = {'type': repo_type, 'path': source}
            if bind_mounts:
                config['bindmounts ' + name] = bind_mounts
            config.save()

    def clone(self, repo_name, path, snapshot_id):
        if path in self._get_directory_map():
            raise Exception('mount path %s already exists.' % path)

        # Make sure that path is a valid destination for the clone.
        st = None
        try:
            st = os.stat(path)
        except OSError as ex:
            if ex.errno == errno.ENOENT:
                # Note that this could also throw if path is /a/b/c and /a
                # exists, but it is a file.
                util.mkdir_p(path)
            else:
                raise

        # Note that st will be None if `mkdir_p` was run in the catch block.
        if st:
            if stat.S_ISDIR(st.st_mode):
                # If an existing directory was specified, then verify it is
                # empty.
                if len(os.listdir(path)) > 0:
                    raise OSError(errno.ENOTEMPTY, os.strerror(errno.ENOTEMPTY),
                                  path)
            else:
                # Throw because it exists, but it is not a directory.
                raise OSError(errno.ENOTDIR, os.strerror(errno.ENOTDIR), path)

        # Create client directory
        dir_name = hashlib.sha1(repo_name.encode('utf-8')).hexdigest()
        client_dir = os.path.join(self._get_clients_dir(), dir_name)
        util.mkdir_p(client_dir)

        # Store repository name in local edenrc config file
        self._store_repo_name(client_dir, repo_name)

        # Store snapshot ID
        if snapshot_id:
            client_snapshot = os.path.join(client_dir, SNAPSHOT)
            with open(client_snapshot, 'w') as f:
                f.write(snapshot_id + '\n')
        else:
            raise Exception('snapshot id not provided')

        # Create bind mounts directories
        repo_data = self.get_repo_data(repo_name)
        bind_mounts_dir = os.path.join(client_dir, 'bind-mounts')
        util.mkdir_p(bind_mounts_dir)
        for mount in repo_data['bind-mounts']:
            util.mkdir_p(os.path.join(bind_mounts_dir, mount))

        # Create local storage directory
        self._get_or_create_write_dir(dir_name)

        # Prepare to mount
        mount_info = eden_ttypes.MountInfo(mountPoint=path,
                                           edenClientPath=client_dir,
                                           homeDir=self._home_dir)
        with self.get_thrift_client() as client:
            client.mount(mount_info)

        # Add mapping of mount path to client directory in config.json
        self._add_path_to_directory_map(path, dir_name)

    def unmount(self, path):
        with self.get_thrift_client() as client:
            client.unmount(path)
        shutil.rmtree(self._get_client_dir_for_mount_point(path))
        self._remove_path_from_directory_map(path)

    def check_health(self):
        '''
        Get the status of the edenfs daemon.

        Returns a HealthStatus object containing health information.
        '''
        pid = None
        status = fb_status.DEAD
        try:
            with self.get_thrift_client() as client:
                pid = client.getPid()
                status = client.getStatus()
        except eden.thrift.EdenNotRunningError:
            return HealthStatus(fb_status.DEAD, pid=None,
                                detail='edenfs not running')
        except thrift.Thrift.TException as ex:
            detail = 'error talking to edenfs: ' + str(ex)
            return HealthStatus(status, pid, detail)

        status_name = fb_status._VALUES_TO_NAMES.get(status)
        detail = 'edenfs running (pid {}); status is {}'.format(
            pid, status_name)
        return HealthStatus(status, pid, detail)

    def spawn(self,
              daemon_binary,
              extra_args=None,
              gdb=False,
              foreground=False):
        '''
        Start edenfs.

        If foreground is True this function never returns (edenfs is exec'ed
        directly in the current process).

        Otherwise, this function waits for edenfs to become healthy, and
        returns a HealthStatus object.  On error an exception will be raised.
        '''
        # Check to see if edenfs is already running
        health_info = self.check_health()
        if health_info.is_healthy():
            raise EdenStartError('edenfs is already running (pid {})'.format(
                health_info.pid))

        # Run the eden server.
        cmd = [daemon_binary, '--edenDir', self._config_dir, ]
        if gdb:
            cmd = ['gdb', '--args'] + cmd
            foreground = True
        if extra_args:
            cmd.extend(extra_args)

        # Run edenfs using sudo, unless we already have root privileges,
        # or the edenfs binary is setuid root.
        if os.geteuid() != 0:
            s = os.stat(daemon_binary)
            if not (s.st_uid == 0 and (s.st_mode & stat.S_ISUID)):
                # We need to run edenfs under sudo
                if ('SANDCASTLE' in os.environ) and os.path.exists(SUDO_HELPER):
                    cmd = [SUDO_HELPER] + cmd
                cmd = ['/usr/bin/sudo', '-E'] + cmd

        eden_env = self._build_eden_environment()

        if foreground:
            # This call does not return
            os.execve(cmd[0], cmd, eden_env)

        # Open the log file
        log_path = self.get_log_path()
        util.mkdir_p(os.path.dirname(log_path))
        log_file = open(log_path, 'a')
        startup_msg = time.strftime('%Y-%m-%d %H:%M:%S: starting edenfs\n')
        log_file.write(startup_msg)

        # Start edenfs
        proc = subprocess.Popen(cmd, env=eden_env, preexec_fn=os.setsid,
                                stdout=log_file, stderr=log_file)
        log_file.close()

        # Wait for edenfs to start
        return self._wait_for_daemon_healthy(proc)

    def _wait_for_daemon_healthy(self, proc):
        '''
        Wait for edenfs to become healthy.
        '''
        def check_health():
            # Check the thrift status
            health_info = self.check_health()
            if health_info.is_healthy():
                return health_info

            # Make sure that edenfs is still running
            status = proc.poll()
            if status is not None:
                if status < 0:
                    msg = 'terminated with signal {}'.format(-status)
                else:
                    msg = 'exit status {}'.format(status)
                raise EdenStartError('edenfs exited before becoming healthy: ' +
                                     msg)

            # Still starting
            return None

        timeout_ex = EdenStartError('timed out waiting for edenfs to become '
                                    'healthy')
        return util.poll_until(check_health, timeout=5, timeout_ex=timeout_ex)

    def get_log_path(self):
        return os.path.join(self._config_dir, 'logs', 'edenfs.log')

    def _build_eden_environment(self):
        # Reset $PATH to the following contents, so that everyone has the
        # same consistent settings.
        path_dirs = [
            '/usr/local/bin',
            '/bin',
            '/usr/bin',
        ]

        eden_env = {
            'PATH': ':'.join(path_dirs),
        }

        # Preserve the following environment settings
        preserve = [
            'USER',
            'LOGNAME',
            'HOME',
            'EMAIL',
            'NAME',
            # When we import data from mercurial, the remotefilelog extension
            # may need to SSH to a remote mercurial server to get the file
            # contents.  Preserve SSH environment variables needed to do this.
            'SSH_AUTH_SOCK',
            'SSH_AGENT_PID',
        ]

        for name, value in os.environ.items():
            # Preserve any environment variable starting with "TESTPILOT_".
            # TestPilot uses a few environment variables to keep track of
            # processes started during test runs, so it can track down and kill
            # runaway processes that weren't cleaned up by the test itself.
            # We want to make sure this behavior works during the eden
            # integration tests.
            if name.startswith('TESTPILOT_'):
                eden_env[name] = value
            elif name in preserve:
                eden_env[name] = value
            else:
                # Drop any environment variable not matching the above cases
                pass

        return eden_env

    def get_or_create_path_to_rocks_db(self):
        rocks_db_dir = os.path.join(self._config_dir, ROCKS_DB_DIR)
        return util.mkdir_p(rocks_db_dir)

    def _store_repo_name(self, client_dir, repo_name):
        config_path = os.path.join(client_dir, LOCAL_CONFIG)
        with ConfigUpdater(config_path) as config:
            config['repository'] = {'name': repo_name}
            config.save()

    def _get_repo_name(self, client_dir):
        config = os.path.join(client_dir, LOCAL_CONFIG)
        parser = configparser.ConfigParser()
        parser.read(config)
        name = parser.get('repository', 'name')
        if name:
            return name
        raise Exception('could not find repository for %s' % client_dir)

    def _get_directory_map(self):
        '''
        Parse config.json which holds a mapping of mount paths to their
        respective client directory and return contents in a dictionary.
        '''
        directory_map = os.path.join(self._config_dir, CONFIG_JSON)
        if os.path.isfile(directory_map):
            with open(directory_map) as f:
                return json.load(f)
        return {}

    def _add_path_to_directory_map(self, path, dir_name):
        config_data = self._get_directory_map()
        if path in config_data:
            raise Exception('mount path %s already exists.' % path)
        config_data[path] = dir_name
        self._write_directory_map(config_data)

    def _remove_path_from_directory_map(self, path):
        config_data = self._get_directory_map()
        if path in config_data:
            del config_data[path]
            self._write_directory_map(config_data)

    def _write_directory_map(self, config_data):
        directory_map = os.path.join(self._config_dir, CONFIG_JSON)
        with open(directory_map, 'w') as f:
            json.dump(config_data, f, indent=2, sort_keys=True)
            f.write('\n')

    def _get_client_dir_for_mount_point(self, path):
        config_data = self._get_directory_map()
        if path not in config_data:
            raise Exception('could not find mount path %s' % path)
        return os.path.join(self._get_clients_dir(), config_data[path])

    def _get_clients_dir(self):
        return os.path.join(self._config_dir, CLIENTS_DIR)

    def _get_or_create_write_dir(self, dir_name):
        ''' Returns the local storage directory that is used to
            hold writes that are not part of a snapshot '''
        local_dir = os.path.join(self._get_clients_dir(),
                                 dir_name, 'local')
        return util.mkdir_p(local_dir)


class HealthStatus(object):
    def __init__(self, status, pid, detail):
        self.status = status
        self.pid = pid  # The process ID, or None if not running
        self.detail = detail  # a human-readable message

    def is_healthy(self):
        return self.status == fb_status.ALIVE


class ConfigUpdater(object):
    '''
    A helper class to safely update an eden config file.

    This acquires a lock on the config file, reads it in, and then provide APIs
    to save it back.  This ensures that another process cannot change the file
    in between the time that we read it and when we write it back.

    This also saves the file to a temporary name first, then renames it into
    place, so that the main config file is always in a good state, and never
    has partially written contents.
    '''
    def __init__(self, path):
        self.path = path
        self._lock_path = self.path + '.lock'
        self._lock_file = None
        self.config = configparser.ConfigParser()
        self.config.read(self.path)

        # Acquire a lock.
        # This makes sure that another process can't modify the config in the
        # middle of a read-modify-write operation.  (We can't stop a user
        # from manually editing the file while we work, but we can stop
        # other eden CLI processes.)
        self._acquire_lock()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, exc_traceback):
        self.close()

    def __del__(self):
        self.close()

    def sections(self):
        return self.config.sections()

    def __getitem__(self, key):
        return self.config[key]

    def __setitem__(self, key, value):
        self.config[key] = value

    def _acquire_lock(self):
        while True:
            self._lock_file = open(self._lock_path, 'w+')
            fcntl.flock(self._lock_file.fileno(), fcntl.LOCK_EX)
            # The original creator of the lock file will unlink it when
            # it is finished.  Make sure we grab the lock on the file still on
            # disk, and not an unlinked file.
            st1 = os.fstat(self._lock_file.fileno())
            st2 = os.lstat(self._lock_path)
            if st1.st_dev == st2.st_dev and st1.st_ino == st2.st_ino:
                # We got the real lock
                return

            # We acquired a lock on an old deleted file.
            # Close it, and try to acquire the current lock file again.
            self._lock_file.close()
            self._lock_file = None
            continue

    def _unlock(self):
        # Remove the file on disk before we unlock it.
        # This way processes currently waiting in _acquire_lock() that already
        # opened our lock file will see that it isn't the current file on disk
        # once they acquire the lock.
        os.unlink(self._lock_path)
        self._lock_file.close()
        self._lock_file = None

    def close(self):
        if self._lock_file is not None:
            self._unlock()

    def save(self):
        if self._lock_file is None:
            raise Exception('Cannot save the config without holding the lock')

        try:
            st = os.stat(self.path)
            perms = (st.st_mode & 0o777)
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise
            perms = 0o644

        # Write the contents to a temporary file first, then atomically rename
        # it to the desired destination.  This makes sure the .edenrc file
        # always has valid contents at all points in time.
        prefix = HOME_CONFIG + '.tmp.'
        dirname = os.path.dirname(self.path)
        tmpf = tempfile.NamedTemporaryFile('w', dir=dirname, prefix=prefix,
                                           delete=False)
        try:
            self.config.write(tmpf)
            tmpf.close()
            os.chmod(tmpf.name, perms)
            os.rename(tmpf.name, self.path)
        except BaseException:
            # Remove temporary file on error
            try:
                os.unlink(tmpf.name)
            except Exception:
                pass
            raise


def _verify_mount_point(mount_point):
    if os.path.isdir(mount_point):
        return
    parent_dir = os.path.dirname(mount_point)
    if os.path.isdir(parent_dir):
        os.mkdir(mount_point)
    else:
        raise Exception(
            ('%s must be a directory in order to mount a client at %s. ' +
             'If this is the correct location, run `mkdir -p %s` to create ' +
             'the directory.') % (parent_dir, mount_point, parent_dir))
