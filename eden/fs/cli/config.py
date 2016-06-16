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

    def spawn(self,
              daemon_binary,
              debug=False,
              gdb=False,
              preserve_environment=False):
        '''Note that this method will not exit until the user kills the program.
        '''
        # Run the eden server.
        cmd = [daemon_binary, '--edenDir', self._config_dir, ]
        if gdb:
            cmd = ['gdb', '--args'] + cmd
        if debug:
            cmd.append('--debug')

        # Run edenfs using sudo, unless we already have root privileges,
        # or the edenfs binary is setuid root.
        if os.geteuid() != 0:
            s = os.stat(daemon_binary)
            if not (s.st_uid == 0 and (s.st_mode & stat.S_ISUID)):
                # Run edenfs under sudo
                # sudo will generally spawn edenfs as a separate child process,
                # rather than just exec()ing it.  Therefore our immediate child
                # will still have root privileges, so we don't have permissions
                # to kill it.  We have permissions to kill the main edenfs
                # process which drops privileges, but we don't know its process
                # ID.
                sudo_args = ['sudo']
                if preserve_environment:
                    sudo_args.append('-E')
                cmd = sudo_args + cmd
            elif os.geteuid() == 0 and not preserve_environment:
                # TODO: Reset the environment when running cmd.
                pass
        subprocess.Popen(cmd, preexec_fn=os.setsid)

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


def _get_or_create_dir(path):
    '''Performs `mkdir -p <path>` and returns the path.'''
    try:
        os.makedirs(path)
    except OSError as e:
        if e.errno != errno.EEXIST:
            raise
    return path
