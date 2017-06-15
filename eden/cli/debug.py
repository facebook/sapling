#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import argparse
import binascii
import os
import stat
import sys
from typing import Tuple

from . import cmd_util


def get_mount_path(path: str) -> Tuple[str, str]:
    '''
    Given a path inside an eden mount, find the path to the eden root.

    Returns a tuple of (eden_mount_path, relative_path)
    where relative_path is the path such that
    os.path.join(eden_mount_path, relative_path) refers to the same file as the
    original input path.
    '''
    # TODO: This will probably be easier to do using the special .eden
    # directory, once the diff adding .eden lands.
    current_path = os.path.realpath(path)
    rel_path = ''
    while True:
        # For now we simply assume that the first mount point we come across is
        # the eden mount point.  This doesn't handle bind mounts inside the
        # eden mount, but that's fine for now.
        if os.path.ismount(current_path):
            rel_path = os.path.normpath(rel_path)
            if rel_path == '.':
                rel_path = ''
            return (current_path, rel_path)

        parent, basename = os.path.split(current_path)
        if parent == current_path:
            raise Exception('eden mount point not found')

        current_path = parent
        rel_path = os.path.join(basename, rel_path)


def escape_path(value: bytes) -> str:
    '''
    Take a binary path value, and return a printable string, with special
    characters escaped.
    '''
    def human_readable_byte(b):
        if b < 0x20 or b >= 0x7f:
            return '\\x{:02x}'.format(b)
        elif b == ord(b'\\'):
            return '\\\\'
        return chr(b)
    return ''.join(human_readable_byte(b) for b in value)


def hash_str(value: bytes) -> str:
    '''
    Take a hash as a binary value, and return it represented as a hexadecimal
    string.
    '''
    return binascii.hexlify(value).decode('utf-8')


def parse_object_id(value: str) -> bytes:
    '''
    Parse an object ID as a 40-byte hexadecimal string, and return a 20-byte
    binary value.
    '''
    try:
        binary = binascii.unhexlify(value)
        if len(binary) != 20:
            raise ValueError()
    except ValueError:
        raise ValueError('blob ID must be a 40-byte hexadecimal value')
    return binary


def do_tree(args: argparse.Namespace):
    config = cmd_util.create_config(args)
    mount, rel_path = get_mount_path(args.mount)
    tree_id = parse_object_id(args.id)

    local_only = not args.load
    with config.get_thrift_client() as client:
        entries = client.debugGetScmTree(mount, tree_id,
                                         localStoreOnly=local_only)

    for entry in entries:
        file_type_flags, perms = _parse_mode(entry.mode)
        print('{} {:4o} {:40} {}'.format(
            file_type_flags, perms, hash_str(entry.id),
            escape_path(entry.name)))


def do_blob(args: argparse.Namespace):
    config = cmd_util.create_config(args)
    mount, rel_path = get_mount_path(args.mount)
    blob_id = parse_object_id(args.id)

    local_only = not args.load
    with config.get_thrift_client() as client:
        data = client.debugGetScmBlob(mount, blob_id,
                                      localStoreOnly=local_only)

    sys.stdout.buffer.write(data)


def do_blobmeta(args: argparse.Namespace):
    config = cmd_util.create_config(args)
    mount, rel_path = get_mount_path(args.mount)
    blob_id = parse_object_id(args.id)

    local_only = not args.load
    with config.get_thrift_client() as client:
        info = client.debugGetScmBlobMetadata(mount, blob_id,
                                              localStoreOnly=local_only)

    print('Blob ID: {}'.format(args.id))
    print('Size:    {}'.format(info.size))
    print('SHA1:    {}'.format(hash_str(info.contentsSha1)))


_FILE_TYPE_FLAGS = {
    stat.S_IFREG: 'f',
    stat.S_IFDIR: 'd',
    stat.S_IFLNK: 'l',
}


def _parse_mode(mode: int) -> Tuple[str, int]:
    '''
    Take a mode value, and return a tuple of (file_type, permissions)
    where file type is a one-character flag indicating if this is a file,
    directory, or symbolic link.
    '''
    file_type_str = _FILE_TYPE_FLAGS.get(stat.S_IFMT(mode), '?')
    perms = (mode & 0o7777)
    return file_type_str, perms


def _print_inode_info(inode_info):
    print('{}:'.format(escape_path(inode_info.path)))
    print('  Inode number:  {}'.format(inode_info.inodeNumber))
    print('  Materialized?: {}'.format(inode_info.materialized))
    print('  Object ID:     {}'.format(hash_str(inode_info.treeHash)))
    print('  Entries ({} total):'.format(len(inode_info.entries)))
    for entry in inode_info.entries:
        if entry.loaded:
            loaded_flag = 'L'
        else:
            loaded_flag = '-'

        file_type_str, perms = _parse_mode(entry.mode)
        line = '    {:9} {} {:4o} {} {:40} {}'.format(
            entry.inodeNumber, file_type_str, perms, loaded_flag,
            hash_str(entry.hash), escape_path(entry.name))
        print(line)


def do_inode(args: argparse.Namespace):
    config = cmd_util.create_config(args)
    mount, rel_path = get_mount_path(args.path)
    with config.get_thrift_client() as client:
        results = client.debugInodeStatus(mount, rel_path)

    print('{} loaded TreeInodes'.format(len(results)))
    for inode_info in results:
        _print_inode_info(inode_info)


def setup_argparse(parser: argparse.ArgumentParser):
    subparsers = parser.add_subparsers(dest='subparser_name')

    parser = subparsers.add_parser(
        'tree', help='Show eden\'s data for a source control tree')
    parser.add_argument('-L', '--load',
                        action='store_true', default=False,
                        help='Load data from the backing store if necessary')
    parser.add_argument('mount', help='The eden mount point path.')
    parser.add_argument('id', help='The tree ID')
    parser.set_defaults(func=do_tree)

    parser = subparsers.add_parser(
        'blob', help='Show eden\'s data for a source control blob')
    parser.add_argument('-L', '--load',
                        action='store_true', default=False,
                        help='Load data from the backing store if necessary')
    parser.add_argument('mount', help='The eden mount point path.')
    parser.add_argument('id', help='The blob ID')
    parser.set_defaults(func=do_blob)

    parser = subparsers.add_parser(
        'blobmeta',
        help='Show eden\'s metadata about a source control blob')
    parser.add_argument('-L', '--load',
                        action='store_true', default=False,
                        help='Load data from the backing store if necessary')
    parser.add_argument('mount', help='The eden mount point path.')
    parser.add_argument('id', help='The blob ID')
    parser.set_defaults(func=do_blobmeta)

    parser = subparsers.add_parser(
        'inode', help='Show data about loaded inodes')
    parser.add_argument(
        'path',
        help='The path to the eden mount point path.  If a subdirectory inside '
        'a mount point is specified, only data about inodes under the '
        'specified subdirectory will be reported.')
    parser.set_defaults(func=do_inode)
