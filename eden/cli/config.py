#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import binascii
import collections
import configparser
import errno
import fcntl
import itertools
import json
import os
import shutil
import stat
import subprocess
import tempfile
import time
import toml

from . import configinterpolator, util
from .util import print_stderr
import eden.thrift
import facebook.eden.ttypes as eden_ttypes
from fb303.ttypes import fb_status
import thrift
from typing import Optional

# Use --etcEdenDir to change the value used for a given invocation
# of the eden cli.
DEFAULT_ETC_EDEN_DIR = '/etc/eden'
# These are INI files that hold config data.
# CONFIG_DOT_D is relative to DEFAULT_ETC_EDEN_DIR, or whatever the
# effective value is for that path
CONFIG_DOT_D = 'config.d'
# USER_CONFIG is relative to the HOME dir for the user
USER_CONFIG = '.edenrc'

# These paths are relative to the user's client directory.
LOCK_FILE = 'lock'
CLIENTS_DIR = 'clients'
STORAGE_DIR = 'storage'
ROCKS_DB_DIR = os.path.join(STORAGE_DIR, 'rocks-db')
CONFIG_JSON = 'config.json'

# These are files in a client directory.
CLONE_SUCCEEDED = 'clone-succeeded'
MOUNT_CONFIG = 'config.toml'
SNAPSHOT = 'SNAPSHOT'
SNAPSHOT_MAGIC = b'eden\x00\x00\x00\x01'

# In our test environment, when we need to run as root, we
# may need to launch via a helper script that is whitelisted
# by the local sudo configuration.
SUDO_HELPER = '/var/www/scripts/testinfra/run_eden.sh'


class EdenStartError(Exception):
    pass


class UsageError(Exception):
    pass


class ClientConfig:
    '''Configuration for a client. A client stores its config in config.toml
    under ~/local/.eden/clients/.

    - path real path where the true repo resides on disk
    - scm_type "hg" or "git"
    - hooks_path path to where the hooks scripts are for the repo
    - bind_mounts dict where keys are private pathnames under ~/.eden where the
      files are actually stored and values are the relative pathnames in the
      EdenFS mount that maps to them.
    '''
    __slots__ = ['path', 'scm_type', 'hooks_path', 'bind_mounts']

    def __init__(self, path, scm_type, hooks_path, bind_mounts):
        self.path = path
        self.scm_type = scm_type
        self.hooks_path = hooks_path
        self.bind_mounts = bind_mounts


class Config:
    def __init__(self, config_dir, etc_eden_dir, home_dir):
        self._config_dir = config_dir
        self._etc_eden_dir = etc_eden_dir
        if not self._etc_eden_dir:
            self._etc_eden_dir = DEFAULT_ETC_EDEN_DIR
        self._user_config_path = os.path.join(home_dir, USER_CONFIG)
        self._home_dir = home_dir

    def _loadConfig(self) -> configparser.ConfigParser:
        ''' to facilitate templatizing a centrally deployed config, we
            allow a limited set of env vars to be expanded.
            ${HOME} will be replaced by the user's home dir,
            ${USER} will be replaced by the user's login name.
            These are coupled with the equivalent code in
            eden/fs/config/ClientConfig.cpp and must be kept in sync.
        '''
        defaults = {'USER': os.environ.get('USER'),
                    'HOME': self._home_dir}
        parser = configparser.ConfigParser(
            interpolation=configinterpolator.EdenConfigInterpolator(defaults))
        parser.read(self.get_rc_files())
        return parser

    def get_rc_files(self):
        result = []
        config_d = os.path.join(self._etc_eden_dir, CONFIG_DOT_D)
        if os.path.isdir(config_d):
            result = os.listdir(config_d)
            result = [os.path.join(config_d, f) for f in result]
            result.sort()
        result.append(self._user_config_path)
        return result

    def get_repository_list(self, parser=None):
        result = []
        if not parser:
            parser = self._loadConfig()
        for section in parser.sections():
            header = section.split(' ')
            if len(header) == 2 and header[0] == 'repository':
                result.append(header[1])
        return sorted(result)

    def get_config_value(self, key):
        parser = self._loadConfig()
        section, option = key.split('.', 1)
        try:
            return parser.get(section, option)
        except (configparser.NoOptionError, configparser.NoSectionError) as exc:
            raise KeyError(str(exc))

    def print_full_config(self):
        parser = self._loadConfig()
        for section in parser.sections():
            print('[%s]' % section)
            for k, v in parser.items(section):
                print('%s=%s' % (k, v))

    def find_config_for_alias(self, alias) -> Optional[ClientConfig]:
        '''Looks through the existing config files and searches for a
        [repository <alias>] section that defines a config:
        - If no such section is found, returns None.
        - If the appropriate section is found, returns a ClientConfig if all of
          the fields for the config data are present and well-formed.
        - Otherwise, throws an Exception.
        '''
        parser = self._loadConfig()
        repository_header = f'repository {alias}'
        if repository_header not in parser:
            return None
        repo_data = parser[repository_header]

        bind_mounts_header = f'bindmounts {alias}'
        if bind_mounts_header in parser:
            # Convert the ConfigParser section into a dict so it is JSON
            # serializable for the `eden info` command.
            bind_mounts = dict(parser[bind_mounts_header].items())
        else:
            bind_mounts = {}

        if 'type' not in repo_data:
            raise Exception(f'repository "{alias}" missing key "type".')
        elif 'path' not in repo_data:
            raise Exception(f'repository "{alias}" missing key "path".')

        hooks_path = repo_data.get('hooks')
        if hooks_path is None:
            hooks_path = self.get_default_hooks_path()

        return ClientConfig(repo_data['path'], repo_data['type'], hooks_path,
                            bind_mounts)

    def get_default_hooks_path(self) -> str:
        return os.path.join(self._etc_eden_dir, 'hooks')

    def create_no_such_repository_exception(self, name: str) -> Exception:
        '''Creates an exception that says no repository is configured with the
        specified name and suggests other repos that are defined in this Config.
        '''
        repos = []
        prefix = 'repository '
        config = self._loadConfig()
        for key in config:
            if key.startswith(prefix):
                repos.append(key[len(prefix):])
        msg = f'No repository configured named "{name}".'
        if repos:
            repos.sort()
            all_repos = ', '.join(map(lambda r: f'"{r}"', repos))
            msg += f' Try one of: {all_repos}.'
        return Exception(msg)

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
        path = os.path.realpath(path)
        client_dir = self._get_client_dir_for_mount_point(path)
        client_config = self._get_client_config(client_dir)
        snapshot = self._get_snapshot(client_dir)

        return collections.OrderedDict([
            ['bind-mounts', client_config.bind_mounts],
            ['mount', path],
            ['snapshot', snapshot],
            ['client-dir', client_dir],
        ])

    @staticmethod
    def _get_snapshot(client_dir: str) -> str:
        '''Return the hex version of the parent hash in the SNAPSHOT file.'''
        snapshot_file = os.path.join(client_dir, SNAPSHOT)
        with open(snapshot_file, 'rb') as f:
            assert f.read(8) == SNAPSHOT_MAGIC
            return binascii.hexlify(f.read(20)).decode('utf-8')

    def checkout(self, path, snapshot_id):
        '''Switch the active snapshot id for a given client'''
        with self.get_thrift_client() as client:
            client.checkOutRevision(path, snapshot_id)

    def add_repository(self, name, repo_type, source, with_buck=False):
        # Check if repository already exists
        with ConfigUpdater(self._user_config_path) as config:
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

    def clone(self, client_config: ClientConfig, path: str, snapshot_id: str):
        if path in self._get_directory_map():
            raise Exception('''\
mount path %s is already configured (see `eden list`). \
Do you want to run `eden mount %s` instead?''' % (path, path))

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
        clients_dir = self._get_clients_dir()
        util.mkdir_p(clients_dir)  # This directory probably already exists.
        client_dir = self._create_client_dir_for_path(clients_dir, path)

        # Store snapshot ID
        if snapshot_id:
            client_snapshot = os.path.join(client_dir, SNAPSHOT)
            with open(client_snapshot, 'wb') as f:
                f.write(SNAPSHOT_MAGIC)
                f.write(binascii.unhexlify(snapshot_id))
        else:
            raise Exception('snapshot id not provided')

        # Create bind mounts directories
        bind_mounts_dir = os.path.join(client_dir, 'bind-mounts')
        util.mkdir_p(bind_mounts_dir)
        for mount in client_config.bind_mounts:
            util.mkdir_p(os.path.join(bind_mounts_dir, mount))

        config_path = os.path.join(client_dir, MOUNT_CONFIG)
        self._save_client_config(client_config, config_path)

        # Prepare to mount
        mount_info = eden_ttypes.MountInfo(mountPoint=path,
                                           edenClientPath=client_dir)
        with self.get_thrift_client() as client:
            client.mount(mount_info)

        self._run_post_clone_hooks(path, client_dir, client_config)

        # Add mapping of mount path to client directory in config.json
        self._add_path_to_directory_map(path, os.path.basename(client_dir))

    def _create_client_dir_for_path(self, clients_dir, path) -> str:
        '''Tries to create a new subdirectory of clients_dir based on the
        basename of the specified path. Tries appending an increasing sequence
        of integers to the basename if there is a collision until it finds an
        available directory name.
        '''
        basename = os.path.basename(path)
        if basename == '':
            raise Exception('Suspicious attempt to clone into: %s' % path)
        for i in itertools.count(start=0):
            if i == 0:
                dir_name = basename
            else:
                dir_name = f'{basename}-{i}'

            client_dir = os.path.join(clients_dir, dir_name)
            try:
                os.mkdir(client_dir)
                return client_dir
            except OSError as e:
                if e.errno == errno.EEXIST:
                    # A directory with the specified name already exists: try
                    # again with the next candidate name.
                    continue
                raise

    def _run_post_clone_hooks(self, eden_mount_path: str, client_dir: str,
                              client_config: ClientConfig):
        # First, check to see if the post-clone hook has been run successfully
        # before.
        clone_success_path = os.path.join(client_dir, CLONE_SUCCEEDED)
        is_initial_mount = not os.path.isfile(clone_success_path)
        if is_initial_mount:
            post_clone = os.path.join(client_config.hooks_path, 'post-clone')
            snapshot = self._get_snapshot(client_dir)
            try:
                subprocess.run([
                    post_clone,
                    client_config.scm_type,
                    eden_mount_path,
                    client_config.path,
                    snapshot,
                ], pass_fds=[1, 2], check=True)
            except OSError as e:
                if e.errno != errno.ENOENT:
                    # TODO(T13448173): If clone fails, then we should roll back
                    # the mount.
                    raise
                print_stderr(f'Did not run post-clone hook "{post_clone}" for '
                             f'{client_config.path} because it was not found.')

        # "touch" the clone_success_path.
        with open(clone_success_path, 'a'):
            os.utime(clone_success_path, None)

    def _save_client_config(self, client_config: ClientConfig, config_path: str):
        # Store information about the mount in the config.toml file.
        config_data = {
            'repository': {
                'path': client_config.path,
                'type': client_config.scm_type,
                'hooks': client_config.hooks_path,
            },
            'bind-mounts': client_config.bind_mounts,
        }
        with open(config_path, 'w') as f:
            toml.dump(config_data, f)

    # TODO(mbolin): Delete this code in December 2017: Let's just assume
    # everyone has been migrated by then.
    def migrate_internal_edenrc_files_to_config_toml_files(self):
        '''If a client has a legacy edenrc file under ~/local/.eden/clients/*/,
        migrate it to a config.toml file.'''
        clients_dir = self._get_clients_dir()
        if not os.path.isdir(clients_dir):
            return
        for entry in os.listdir(clients_dir):
            edenrc = os.path.join(clients_dir, entry, 'edenrc')
            if not os.path.isfile(edenrc):
                continue

            parser = configparser.ConfigParser()
            parser.read(edenrc)
            repo_name = parser.get('repository', 'name')
            client_config = self.get_repo_config(repo_name)
            config_path = os.path.join(clients_dir, entry, MOUNT_CONFIG)
            self._save_client_config(client_config, config_path)
            os.remove(edenrc)

    def mount(self, path):
        # Load the config info for this client, to make sure we
        # know about the client.
        path = os.path.realpath(path)
        client_dir = self._get_client_dir_for_mount_point(path)

        # Call _get_client_config() for the side-effect of it raising an
        # Exception if the config is in an invalid state.
        self._get_client_config(client_dir)

        # Make sure the mount path exists
        util.mkdir_p(path)

        # Check if it is already mounted.
        try:
            root = os.path.join(path, '.eden', 'root')
            target = os.readlink(root)
            if target == path:
                print_stderr('ERROR: Mount point in use! '
                             '{} is already mounted by Eden.', path)
                return 1
            else:
                # If we are here, MOUNT/.eden/root is a symlink, but it does not
                # point to MOUNT. This suggests `path` is a subdirectory of an
                # existing mount, though we should never reach this point
                # because _get_client_dir_for_mount_point() above should have
                # already thrown an exception. We return non-zero here just in
                # case.
                print_stderr('ERROR: Mount point in use! '
                             '{} is already mounted by Eden as part of {}.',
                             path, root)
                return 1
        except OSError as ex:
            err = ex.errno
            if err != errno.ENOENT and err != errno.EINVAL:
                raise

        # Ask eden to mount the path
        mount_info = eden_ttypes.MountInfo(mountPoint=path,
                                           edenClientPath=client_dir)
        with self.get_thrift_client() as client:
            client.mount(mount_info)

    def unmount(self, path, delete_config):
        path = os.path.realpath(path)
        with self.get_thrift_client() as client:
            client.unmount(path)

        if delete_config:
            shutil.rmtree(self._get_client_dir_for_mount_point(path))
            self._remove_path_from_directory_map(path)

            # Delete the now empty mount point
            os.rmdir(path)

    def check_health(self) -> 'HealthStatus':
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
            # It is possible that the edenfs process is running, but the Thrift
            # server is not running. This could be during the startup, shutdown,
            # or takeover of the edenfs process. As a backup to requesting the
            # PID from the Thrift server, we read it from the lockfile and try
            # to deduce the current status of Eden.
            return self._check_health_using_lockfile()
        except thrift.Thrift.TException as ex:
            detail = 'error talking to edenfs: ' + str(ex)
            return HealthStatus(status, pid, detail)

        status_name = fb_status._VALUES_TO_NAMES.get(status)
        detail = 'edenfs running (pid {}); status is {}'.format(
            pid, status_name)
        return HealthStatus(status, pid, detail)

    def _check_health_using_lockfile(self) -> 'HealthStatus':
        '''Make a best-effort to produce a HealthStatus based on the PID in the
        Eden lockfile.
        '''
        lockfile = os.path.join(self._config_dir, LOCK_FILE)
        try:
            with open(lockfile, 'r') as f:
                lockfile_contents = f.read()
            pid = lockfile_contents.rstrip()
            int(pid)  # Throw if this does not parse as an integer.
        except Exception:
            # If we cannot read the PID from the lockfile for any reason, return
            # DEAD.
            return self._create_dead_health_status()

        try:
            stdout = subprocess.check_output(['ps', '-p', pid, '-o', 'comm='])
        except subprocess.CalledProcessError:
            # If there is no process with the specified id, return DEAD.
            return self._create_dead_health_status()

        # Use heuristics to determine that the PID in the lockfile is associated
        # with an edenfs process as it is possible that edenfs is no longer
        # running and the PID in the lockfile has been assigned to a new process
        # unrelated to Eden.
        comm = stdout.rstrip().decode('utf8')
        # Note that the command may be just "edenfs" rather than a path, but it
        # works out fine either way.
        if os.path.basename(comm) == 'edenfs':
            return HealthStatus(fb_status.STOPPED, int(pid),
                                'Eden\'s Thrift server does not appear to be '
                                'running, but the process is still alive ('
                                'PID=%s).' % pid)
        else:
            return self._create_dead_health_status()

    def _create_dead_health_status(self) -> 'HealthStatus':
        return HealthStatus(fb_status.DEAD, pid=None,
                            detail='edenfs not running')

    def spawn(self,
              daemon_binary,
              extra_args=None,
              gdb=False,
              gdb_args=None,
              strace_file=None,
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

        if gdb and strace_file is not None:
            raise EdenStartError('cannot run eden under gdb and '
                                 'strace together')

        # Run the eden server.
        cmd = [daemon_binary, '--edenDir', self._config_dir,
               '--etcEdenDir', self._etc_eden_dir,
               '--configPath', self._user_config_path, ]
        if gdb:
            gdb_args = gdb_args or []
            cmd = ['gdb'] + gdb_args + ['--args'] + cmd
            foreground = True
        if strace_file is not None:
            cmd = ['strace', '-fttT', '-o', strace_file] + cmd
        if extra_args:
            cmd.extend(extra_args)

        eden_env = self._build_eden_environment()

        # Run edenfs using sudo, unless we already have root privileges,
        # or the edenfs binary is setuid root.
        if os.geteuid() != 0:
            s = os.stat(daemon_binary)
            if not (s.st_uid == 0 and (s.st_mode & stat.S_ISUID)):
                # We need to run edenfs under sudo
                sudo_cmd = ['/usr/bin/sudo']
                # Add environment variable settings
                # Depending on the sudo configuration, these may not
                # necessarily get passed through automatically even when
                # using "sudo -E".
                for key, value in eden_env.items():
                    sudo_cmd.append('%s=%s' % (key, value))

                if ('SANDCASTLE' in os.environ) and os.path.exists(SUDO_HELPER):
                    cmd = [SUDO_HELPER] + cmd
                cmd = sudo_cmd + cmd

        if foreground:
            # This call does not return
            os.execve(cmd[0], cmd, eden_env)

        # Open the log file
        log_path = self.get_log_path()
        util.mkdir_p(os.path.dirname(log_path))
        with open(log_path, 'a') as log_file:
            startup_msg = time.strftime('%Y-%m-%d %H:%M:%S: starting edenfs\n')
            log_file.write(startup_msg)

            # Start edenfs
            proc = subprocess.Popen(cmd, env=eden_env, preexec_fn=os.setsid,
                                    stdout=log_file, stderr=log_file)

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

    def get_log_path(self) -> str:
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
            'KRB5CCNAME',
        ]

        for name, value in os.environ.items():
            # Preserve any environment variable starting with "TESTPILOT_".
            # TestPilot uses a few environment variables to keep track of
            # processes started during test runs, so it can track down and kill
            # runaway processes that weren't cleaned up by the test itself.
            # We want to make sure this behavior works during the eden
            # integration tests.
            # Similarly, we want to preserve EDENFS_ env vars which are
            # populated by our own test infra to relay paths to important
            # build artifacts in our build tree.
            if name.startswith('TESTPILOT_') or name.startswith('EDENFS_'):
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

    def _get_client_config(self, client_dir) -> ClientConfig:
        '''Returns ClientConfig or raises an Exception if the config.toml
        under the client_dir is not properly formatted or does not exist.
        '''
        config_toml = os.path.join(client_dir, MOUNT_CONFIG)
        with open(config_toml, 'r') as f:
            config = toml.load(f)
        repository = config.get('repository')
        if not isinstance(repository, dict):
            raise Exception(f'{config_toml} is missing [repository]')

        def get_field(key: str):
            value = repository.get(key)
            if not isinstance(value, str):
                raise Exception(f'{config_toml} is missing {key} in '
                                '[repository]')
            return value

        path = get_field('path')
        scm_type = get_field('type')
        hooks_path = get_field('hooks')

        bind_mounts = {}
        bind_mounts_dict = config.get('bind-mounts')
        if bind_mounts_dict is not None:
            if not isinstance(bind_mounts_dict, dict):
                raise Exception(f'{config_toml} has an invalid '
                                '[bind-mounts] section')
            for key, value in bind_mounts_dict.items():
                if not isinstance(value, str):
                    raise Exception(f'{config_toml} has invalid value in '
                                    f'[bind-mounts] for {key}: {value} '
                                    '(string expected)')
                bind_mounts[key] = value

        return ClientConfig(path, scm_type, hooks_path, bind_mounts)

    def get_client_config_for_path(self, path: str) -> Optional[ClientConfig]:
        directory_map = self._get_directory_map()
        client_dir_component = directory_map.get(path)
        if client_dir_component is None:
            return None
        client_dir = os.path.join(self._get_clients_dir(), client_dir_component)
        return self._get_client_config(client_dir)

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
        # The caller is responsible for making sure the path is already
        # a normalized, absolute path.
        assert os.path.isabs(path)

        config_data = self._get_directory_map()
        if path not in config_data:
            raise Exception('could not find mount path %s' % path)
        return os.path.join(self._get_clients_dir(), config_data[path])

    def _get_clients_dir(self):
        return os.path.join(self._config_dir, CLIENTS_DIR)


class HealthStatus(object):
    def __init__(self, status: fb_status, pid: Optional[int], detail: str) -> None:
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

        # Acquire a lock.
        # This makes sure that another process can't modify the config in the
        # middle of a read-modify-write operation.  (We can't stop a user
        # from manually editing the file while we work, but we can stop
        # other eden CLI processes.)
        self._acquire_lock()
        self.config.read(self.path)

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
        prefix = USER_CONFIG + '.tmp.'
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
