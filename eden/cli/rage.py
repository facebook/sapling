#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import eden.thrift
import getpass
import io
import socket
import subprocess
import thrift

from . import debug as debug_mod
from typing import IO


def print_diagnostic_info(config, args, out: IO[bytes]):
    out.write(b'User                    : %s\n' % getpass.getuser().encode())
    out.write(b'Hostname                : %s\n' % socket.gethostname().encode())
    print_rpm_version(out)
    print_exported_values(config, out)
    print_tail_of_log_file(config.get_log_path(), out)
    print_running_eden_process(out)

    out.write(b'\nList of mount points:\n')
    mountpoint_paths = []
    for key in sorted(config.get_mount_paths()):
        key = key.encode()
        out.write(key)
        mountpoint_paths.append(key)
    for key, val in config.get_all_client_config_info().items():
        out.write(b'\nMount point info for path %s:\n' % key.encode())
        for k, v in val.items():
            out.write('{:>10} : {}\n'.format(k, v).encode())
    for path in mountpoint_paths:
        out.write(b'\nInode information for path %s:\n' % path)
        args.path = path.decode('utf8')
        debug_mod.do_inode(args, out)


def print_rpm_version(out: IO[bytes]):
    try:
        queryformat = ('%{VERSION}')
        output = subprocess.check_output(
            ['rpm', '-q', '--qf', queryformat, 'fb-eden']
        )
        out.write(b'Rpm Version             : %s\n' % output)
    except Exception as e:
        out.write(b'Error getting the Rpm version : %s\n' % str(e).encode())


def print_exported_values(config, out: IO[bytes]):
    try:
        with config.get_thrift_client() as client:
            data = client.getExportedValues()
            out.write(
                b'Package Version         : %s\n' %
                data['build_package_version'].encode()
            )
            out.write(
                b'Build Revision          : %s\n' %
                data['build_revision'].encode()
            )
            out.write(
                b'Build Upstream Revision : %s\n' %
                data['build_upstream_revision'].encode()
            )
    except eden.thrift.EdenNotRunningError:
        out.write(b'edenfs not running\n')
    except thrift.Thrift.TException as e:
        out.write(b'error talking to edenfs: %s\n' % str(e).encode())


def print_tail_of_log_file(path: str, out: IO[bytes]):
    try:
        out.write(b'\nMost recent Eden logs:\n')
        LOG_AMOUNT = 20 * 1024
        with open(path, 'rb') as logfile:
            size = logfile.seek(0, io.SEEK_END)
            logfile.seek(max(0, size - LOG_AMOUNT), io.SEEK_SET)
            data = logfile.read()
            out.write(data)
    except Exception as e:
        out.write(b'Error reading the log file: %s\n' % str(e).encode())


def print_running_eden_process(out: IO[bytes]):
    try:
        out.write(b'\nList of running Eden processes:\n')
        output = subprocess.check_output(
            ['ps', '-eo', 'pid,ppid,comm,start_time,etime']
        )
        output = output.decode()
        lines = output.split('\n')
        format_str = '{:>20} {:>20} {:>10} {:>20} {:>20}\n'
        out.write(
            format_str.format(
                'Pid', 'PPid', 'Command', 'Start Time', 'Elapsed Time'
            ).encode()
        )
        for line in lines:
            if 'edenfs' in line:
                word = line.split()
                out.write(format_str.format(*word).encode())
    except Exception as e:
        out.write(b'Error getting the eden processes: %s\n' % str(e).encode())
