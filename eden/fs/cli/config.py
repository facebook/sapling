# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import (absolute_import, division,
                        print_function, unicode_literals)

import collections
import errno
import json
import os
import signal
import stat
import subprocess
from facebook.eden import EdenService
import facebook.eden.ttypes as eden_ttypes
from eden.thrift import create_thrift_client

# These paths are relative to the user's client directory.
CLIENTS_DIR = 'clients'
STORAGE_DIR = 'storage'
ROCKS_DB_DIR = os.path.join(STORAGE_DIR, 'rocks-db')

# These are files in a client directory.
CONFIG_JSON = 'config.json'
SNAPSHOT = 'SNAPSHOT'


class Config:
    def __init__(self, config_dir):
        self._config_dir = config_dir

    def get_client_names(self):
        clients_dir = self._get_clients_dir()
        if not os.path.isdir(clients_dir):
            return []
        else:
            clients = []
            for entry in os.listdir(clients_dir):
                if (os.path.isdir(os.path.join(clients_dir, entry))):
                    clients.append(entry)
            return clients

    def get_all_client_config_info(self):
        info = {}
        for client in self.get_client_names():
            info[client] = self.get_client_info(client)

        return info

    def get_thrift_client(self):
        return create_thrift_client(self._config_dir)

    def get_client_info(self, name):
        client_dir = os.path.join(self._get_clients_dir(), name)
        if not os.path.isdir(client_dir):
            raise Exception('Error: no such client "%s"' % name)

        client_config = os.path.join(client_dir, CONFIG_JSON)
        config_data = None
        with open(client_config) as f:
            config_data = json.load(f)

        snapshot_file = os.path.join(client_dir, SNAPSHOT)
        snapshot = open(snapshot_file).read().strip()

        return collections.OrderedDict([
            ['bind-mounts', config_data['bind-mounts']],
            ['mount', config_data['mount']],
            ['snapshot', snapshot],
            ['client-dir', client_dir],
        ])

    def create_client(self, name, mount_point, snapshot_id,
                      repo_type, repo_source,
                      with_buck=False):
        '''
        Creates a new client directory with the config.yaml and SNAPSHOT files.
        '''
        _verify_mount_point(mount_point)
        client_dir = os.path.join(self._get_clients_dir(), name)
        if os.path.isdir(client_dir):
            raise Exception('Error: client %s already exists.' % name)

        os.makedirs(client_dir)
        client_config = os.path.join(client_dir, CONFIG_JSON)

        bind_mounts = {}
        bind_mounts_dir = os.path.join(client_dir, 'bind-mounts')
        os.makedirs(bind_mounts_dir)

        if with_buck:
            # TODO: This eventually needs to be more configurable.
            # Some of our repositories need multiple buck-out directories
            # in various subdirectories, rather than a single buck-out
            # directory at the root.
            bind_mount_name = 'buck-out'
            bind_mounts[bind_mount_name] = 'buck-out'
            os.makedirs(os.path.join(bind_mounts_dir, bind_mount_name))

        config_data = {
            'bind-mounts': bind_mounts,
            'mount': mount_point,
            'repo_type': repo_type,
            'repo_source': repo_source,
        }
        with open(client_config, 'w') as f:
            json.dump(config_data, f, indent=2, sort_keys=True)
            f.write('\n')  # json.dump() does not print a trailing newline.

        # TODO(mbolin): We need to decide what the protocol is when a new, empty
        # Eden client is created rather than seeding it with Git or Hg data.
        if snapshot_id:
            client_snapshot = os.path.join(client_dir, SNAPSHOT)
            with open(client_snapshot, 'w') as f:
                f.write(snapshot_id + '\n')

    def checkout(self, name, snapshot_id):
        '''Switch the active snapshot id for a given client'''
        info = self.get_client_info(name)
        client = self.get_thrift_client()
        try:
            client.checkOutRevision(info['mount'], snapshot_id)
        except EdenService.EdenError as ex:
            # str(ex) yields a rather ugly string, this reboxes the
            # exception so that the error message looks nicer in
            # the driver script.
            raise Exception(ex.message)

    def mount(self, name):
        info = self.get_client_info(name)
        mount_point = info['mount']
        _verify_mount_point(mount_point)
        self._get_or_create_write_dir(name)
        mount_info = eden_ttypes.MountInfo(mountPoint=mount_point,
                                           edenClientPath=info['client-dir'])
        client = self.get_thrift_client()
        try:
            client.mount(mount_info)
        except EdenService.EdenError as ex:
            # str(ex) yields a rather ugly string, this reboxes the
            # exception so that the error message looks nicer in
            # the driver script.
            raise Exception(ex.message)

    def unmount(self, name):
        info = self.get_client_info(name)
        mount_point = info['mount']
        client = self.get_thrift_client()
        client.unmount(mount_point)

    def spawn(self, debug=False, gdb=False):
        '''Note that this method will not exit until the user kills the program.
        '''
        def kill_child_process(signum, stack_frame):
            p.send_signal(signum)

        def kill_child_process_group(signum, stack_frame):
            try:
                os.kill(-p.pid, signum)
            except EnvironmentError as ex:
                # Since the child process is started as root, we sometimes can
                # get EPERM when trying to kill its process group.  (This can
                # happen if the privileged helper is still around.)
                #
                # Ignore this error, but re-raise all others.  The privileged
                # helper should die on its own anyway when the main process
                # goes away.
                if ex.errno != errno.EPERM:
                    raise

        # Run the eden server.
        eden_bin = _get_path_to_eden_server()
        cmd = [
            eden_bin,
            '--edenDir', self._config_dir,
        ]
        if gdb:
            cmd = ['gdb', '--args'] + cmd
        if debug:
            cmd.append('--debug')

        # Run edenfs using sudo, unless we already have root privileges,
        # or the edenfs binary is setuid root.
        if os.geteuid() == 0:
            need_sudo = False
        else:
            s = os.stat(eden_bin)
            if s.st_uid == 0 and (s.st_mode & stat.S_ISUID):
                need_sudo = False
            else:
                need_sudo = True

        if need_sudo:
            # Run edenfs under sudo
            # sudo will generally spawn edenfs as a separate child process,
            # rather than just exec()ing it.  Therefore our immediate child
            # will still have root privileges, so we don't have permissions to
            # kill it.  We have permissions to kill the main edenfs process
            # which drops privileges, but we don't know its process ID.
            #
            # Therefore, call os.setsid() in our child before invoking sudo.
            # When we want to kill eden we can just send a signal to the entire
            # child process group.
            sigterm_handler = kill_child_process_group
            # Have to set a SIGINT handler too.  Since edenfs isn't part of our
            # process group it won't get SIGING when Ctrl-C is sent to our
            # terminal.
            sigint_handler = kill_child_process_group
            p = subprocess.Popen(['sudo'] + cmd, preexec_fn=os.setsid)
        else:
            # We can just run edenfs directly.  On SIGTERM we just kill it.
            sigterm_handler = kill_child_process
            # We ignore SIGINT.  Hitting Ctrl-C in the terminal sends SIGINT to
            # our entire process group, so it will automatically go to the main
            # edenfs process too.
            sigint_handler = signal.SIG_IGN
            p = subprocess.Popen(cmd)

        # If we get sent SIGTERM, forward it through to edenfs.
        #
        # If we get sent SIGINT, ignore it, and just keep waiting for edenfs to
        # exit.  SIGINT normally will be sent from a user hitting Ctrl-C on the
        # terminal, in which case Ctrl-C will be sent to everything in our
        # process group (including edenfs), so we don't need to forward SIGINT
        # to it a second time.
        signal.signal(signal.SIGTERM, sigterm_handler)
        signal.signal(signal.SIGINT, sigint_handler)
        return p.wait()

    def get_or_create_path_to_rocks_db(self):
        rocks_db_dir = os.path.join(self._config_dir, ROCKS_DB_DIR)
        return _get_or_create_dir(rocks_db_dir)

    def _get_clients_dir(self):
        return os.path.join(self._config_dir, CLIENTS_DIR)

    def _get_or_create_write_dir(self, client_name):
        ''' Returns the local storage directory that is used to
            hold writes that are not part of a snapshot '''
        local_dir = os.path.join(self._get_clients_dir(),
                                 client_name, 'local')
        return _get_or_create_dir(local_dir)


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


def _get_path_to_eden_server():
    from libfb.parutil import get_file_path
    return get_file_path('eden/fs/cli/eden-server')


def _get_or_create_dir(path):
    '''Performs `mkdir -p <path>` and returns the path.'''
    try:
        os.makedirs(path)
    except OSError as e:
        if e.errno != errno.EEXIST:
            raise
    return path
