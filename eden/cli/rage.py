#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import io
import subprocess
from . import debug as debug_mod
from . import cmd_util
import eden.thrift
import thrift


class Rage:
    def __init__(self, args):
        self._args = args

    def check_diagnostic_info(self):
        mountpoint_paths = []
        config = cmd_util.create_config(self._args)
        get_rpm_version()
        self.print_exported_values(config)
        tail_log_file(config.get_log_path())
        get_running_eden_process()
        print("\nList of mount points:")
        for key in sorted(config.get_mount_paths()):
            print(key)
            mountpoint_paths.append(key)
        for key, val in config.get_all_client_config_info().items():
            print("\nMount point info for path %s : " % key)
            for k, v in val.items():
                print("{:>10} : {}".format(k, v))
        args = self._args
        for path in mountpoint_paths:
            print("\nInode information for path %s:\n" % path)
            args.path = path
            debug_mod.do_inode(args)

    def print_exported_values(self, config):
        try:
            with config.get_thrift_client() as client:
                data = client.getExportedValues()
                print('Package Version        :%s' % data['build_package_version'])
                print('Build Revision         :%s' % data['build_revision'])
                print('Build Upstream Revision:%s' % data['build_upstream_revision'])
        except eden.thrift.EdenNotRunningError:
            print('edenfs not running')
        except thrift.Thrift.TException as ex:
            print('error talking to edenfs: ' + str(ex))


def tail_log_file(path):
    try:
        print('\nMost recent Eden logs:')
        LOG_AMOUNT = 20 * 1024
        with open(path, 'rb') as logfile:
            size = logfile.seek(0, io.SEEK_END)
            logfile.seek(max(0, size - LOG_AMOUNT), io.SEEK_SET)
            data = logfile.read()
            lines = data.decode().split('\n')
            for line in lines:
                print(line)
    except Exception as e:
        print('Error reading the log file %s' % str(e))


def get_running_eden_process():
    try:
        print("\nList of running Eden processes:")
        output = subprocess.check_output([
            'ps', '-eo', 'pid,ppid,comm,start_time,etime'])
        output = output.decode()
        lines = output.split('\n')
        format_str = '{:>20} {:>20} {:>10} {:>20} {:>20}'
        print(format_str.format(
            'Pid', 'PPid', 'Command', 'Start Time', 'Elapsed Time'))
        for line in lines:
            if 'edenfs' in line:
                word = line.split()
                print(format_str.format(*word))
    except Exception as e:
        print('Error getting the eden processes %s' % str(e))


def get_rpm_version():
    try:
        queryformat = ('%{VERSION}')
        output = subprocess.check_output(['rpm', '-q', '--qf', queryformat, 'fb-eden'])
        print('Rpm Version            :%s' % output.decode())
    except Exception as e:
        print('Error getting the Rpm version %s' % str(e))
