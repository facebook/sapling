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
import collections
import os
import stat
import sys
from typing import List, IO, Tuple

from facebook.eden.overlay.ttypes import OverlayDir
import eden.dirstate
from facebook.eden.ttypes import NoValueForKeyError, TimeSpec

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


def do_buildinfo(args: argparse.Namespace, out: IO[bytes] = None):
    if out is None:
        out = sys.stdout.buffer
    config = cmd_util.create_config(args)
    build_info = config.get_server_build_info()
    sorted_build_info = collections.OrderedDict(sorted(build_info.items()))
    for key, value in sorted_build_info.items():
        out.write(f'{key}: {value}\n'.encode())


def do_uptime(args: argparse.Namespace, out: IO[bytes] = None):
    if out is None:
        out = sys.stdout.buffer
    config = cmd_util.create_config(args)
    uptime = config.get_uptime()  # Check if uptime is negative?
    days = uptime.days
    hours, remainder = divmod(uptime.seconds, 3600)
    minutes, seconds = divmod(remainder, 60)
    uptime = '%dd:%02dh:%02dm:%02ds\n' % (days, hours, minutes, seconds)
    out.write(uptime.encode())


def do_hg_copy_map_get_all(args: argparse.Namespace):
    mount, _ = get_mount_path(args.path)
    _parents, _dirstate_tuples, copymap = _get_dirstate_data(mount)
    _print_copymap(copymap)


def _print_copymap(copy_map) -> None:
    copies = [f'{item[1]} -> {item[0]}' for item in copy_map.items()]
    copies.sort()
    for copy in copies:
        print(copy)


def do_hg_dirstate(args: argparse.Namespace) -> None:
    mount, _ = get_mount_path(args.path)
    _parents, dirstate_tuples, copymap = _get_dirstate_data(mount)
    printer = StdoutPrinter()
    entries = list(dirstate_tuples.items())
    print(printer.bold('Non-normal Files (%d):' % len(entries)))
    entries.sort(key=lambda entry: entry[0])  # Sort by key.
    for path, dirstate_tuple in entries:
        _print_hg_nonnormal_file(path, dirstate_tuple, printer)

    print(printer.bold('Copymap (%d):' % len(copymap)))
    _print_copymap(copymap)


def do_hg_get_dirstate_tuple(args: argparse.Namespace):
    mount, rel_path = get_mount_path(args.path)
    _parents, dirstate_tuples, _copymap = _get_dirstate_data(mount)
    dirstate_tuple = dirstate_tuples.get(rel_path)
    printer = StdoutPrinter()
    if dirstate_tuple:
        _print_hg_nonnormal_file(rel_path, dirstate_tuple, printer)
    else:
        config = cmd_util.create_config(args)
        with config.get_thrift_client() as client:
            try:
                entry = client.getManifestEntry(mount, rel_path)
                dirstate_tuple = ('n', entry.mode, 0)
                _print_hg_nonnormal_file(rel_path, dirstate_tuple, printer)
            except NoValueForKeyError:
                print('No tuple for ' + rel_path, file=sys.stderr)
                return 1


def _print_hg_nonnormal_file(
    rel_path, dirstate_tuple, printer: 'StdoutPrinter'
) -> None:
    status = _dirstate_char_to_name(dirstate_tuple[0])
    merge_state = _dirstate_merge_state_to_name(dirstate_tuple[2])

    print(
        f'''\
{printer.green(rel_path)}
    status = {status}
    mode = {oct(dirstate_tuple[1])}
    mergeState = {merge_state}\
'''
    )


def _dirstate_char_to_name(state: str) -> str:
    if state == 'n':
        return 'Normal'
    elif state == 'm':
        return 'NeedsMerging'
    elif state == 'r':
        return 'MarkedForRemoval'
    elif state == 'a':
        return 'MarkedForAddition'
    elif state == '?':
        return 'NotTracked'
    else:
        raise Exception(f'Unrecognized dirstate char: {state}')


def _dirstate_merge_state_to_name(merge_state: int) -> str:
    if merge_state == 0:
        return 'NotApplicable'
    elif merge_state == -1:
        return 'BothParents'
    elif merge_state == -2:
        return 'OtherParent'
    else:
        raise Exception(f'Unrecognized merge_state value: {merge_state}')


def _get_dirstate_data(mount):
    '''Returns a tuple of (parents, dirstate_tuples, copymap).
    On error, returns None.
    '''
    filename = os.path.join(mount, '.hg', 'dirstate')
    with open(filename, 'rb') as f:
        return eden.dirstate.read(f, filename)


def do_inode(args: argparse.Namespace, out: IO[bytes] = None):
    if out is None:
        out = sys.stdout.buffer
    config = cmd_util.create_config(args)
    mount, rel_path = get_mount_path(args.path)
    with config.get_thrift_client() as client:
        results = client.debugInodeStatus(mount, rel_path)

    out.write(b'%d loaded TreeInodes\n' % len(results))
    for inode_info in results:
        _print_inode_info(inode_info, out)


def _print_inode_info(inode_info, out: IO[bytes]):
    out.write(inode_info.path + b'\n')
    out.write(b'  Inode number:  %d\n' % inode_info.inodeNumber)
    out.write(b'  Ref count:     %d\n' % inode_info.refcount)
    out.write(b'  Materialized?: %s\n' % str(inode_info.materialized).encode())
    out.write(b'  Object ID:     %s\n' % hash_str(inode_info.treeHash).encode())
    out.write(b'  Entries (%d total):\n' % len(inode_info.entries))
    for entry in inode_info.entries:
        if entry.loaded:
            loaded_flag = 'L'
        else:
            loaded_flag = '-'

        file_type_str, perms = _parse_mode(entry.mode)
        line = '    {:9} {} {:4o} {} {:40} {}\n'.format(
            entry.inodeNumber, file_type_str, perms, loaded_flag,
            hash_str(entry.hash), escape_path(entry.name))
        out.write(line.encode())


def _load_overlay_tree(overlay_dir: str, inode_number: int) -> OverlayDir:
    from thrift.util import Serializer
    from thrift.protocol import TCompactProtocol

    dir_name = '{:02x}'.format(inode_number % 256)
    overlay_file_path = os.path.join(overlay_dir, dir_name, str(inode_number))
    with open(overlay_file_path, 'rb') as f:
        data = f.read()

    assert data[0:4] == b'OVDR'

    tree_data = OverlayDir()
    protocol_factory = TCompactProtocol.TCompactProtocolFactory()
    Serializer.deserialize(protocol_factory, data[64:], tree_data)
    return tree_data


def _print_overlay_tree(inode_number: int, path: str, tree_data: OverlayDir):
    def hex(binhash):
        return binascii.hexlify(binhash).decode('utf-8')

    print('Inode {}: {}'.format(inode_number, path))
    if not tree_data.entries:
        return
    name_width = max(len(name) for name in tree_data.entries)
    for name, entry in tree_data.entries.items():
        perms = entry.mode & 0o7777
        file_type = stat.S_IFMT(entry.mode)
        if file_type == stat.S_IFREG:
            file_type_flag = 'f'
        elif file_type == stat.S_IFDIR:
            file_type_flag = 'd'
        elif file_type == stat.S_IFLNK:
            file_type_flag = 'l'
        else:
            file_type_flag = '?'

        print('    {:{name_width}s} : {:12d} {} {:04o} {}'.format(
            name, entry.inodeNumber, file_type_flag, perms, hex(entry.hash),
            name_width=name_width))


def _find_overlay_tree_rel(
        overlay_dir: str,
        root: OverlayDir,
        path_parts: List[str]) -> int:
    desired = path_parts[0]
    rest = path_parts[1:]
    for name, entry in root.entries.items():  # noqa: ignore=B007
        if name == desired:
            break
    else:
        raise Exception('path does not exist')

    if stat.S_IFMT(entry.mode) != stat.S_IFDIR:
        raise Exception('path does not not refer to a directory')
    if entry.hash:
        raise Exception('path is not materialized')
    if entry.inodeNumber == 0:
        raise Exception('path is not loaded')

    if rest:
        entry_data = _load_overlay_tree(overlay_dir, entry.inodeNumber)
        return _find_overlay_tree_rel(overlay_dir, entry_data, rest)
    return entry.inodeNumber


def _find_overlay_tree(overlay_dir: str, path: str) -> int:
    assert path
    assert not os.path.isabs(path)

    root = _load_overlay_tree(overlay_dir, 1)
    path_parts = path.split(os.sep)
    return _find_overlay_tree_rel(overlay_dir, root, path_parts)


def _display_overlay(
        args: argparse.Namespace,
        overlay_dir: str,
        inode_number: int,
        path: str,
        level: int = 0):
    data = _load_overlay_tree(overlay_dir, inode_number)
    _print_overlay_tree(inode_number, path, data)

    # If args.depth is negative, recurse forever.
    # Stop if args.depth is non-negative, and level reaches the maximum
    # requested recursion depth.
    if args.depth >= 0 and level >= args.depth:
        return

    for name, entry in data.entries.items():
        if entry.hash or entry.inodeNumber == 0:
            # This entry is not materialized
            continue
        if stat.S_IFMT(entry.mode) != stat.S_IFDIR:
            # Only display data for directories
            continue
        print()
        entry_path = os.path.join(path, name)
        _display_overlay(args, overlay_dir, entry.inodeNumber,
                         entry_path, level + 1)


def do_overlay(args: argparse.Namespace):
    config = cmd_util.create_config(args)
    mount, rel_path = get_mount_path(args.path or os.getcwd())

    # Get the path to the overlay directory for this mount point
    client_dir = config._get_client_dir_for_mount_point(mount)
    overlay_dir = os.path.join(client_dir, 'local')

    if args.number is not None:
        _display_overlay(args, overlay_dir, args.number, '')
    elif rel_path:
        rel_path = os.path.normpath(rel_path)
        inode_number = _find_overlay_tree(overlay_dir, rel_path)
        _display_overlay(args, overlay_dir, inode_number, rel_path)
    else:
        _display_overlay(args, overlay_dir, 1, '/')


def do_getpath(args: argparse.Namespace):
    config = cmd_util.create_config(args)
    mount, _ = get_mount_path(args.path or os.getcwd())

    with config.get_thrift_client() as client:
        inodePathInfo = client.debugGetInodePath(mount, args.number)
    print('%s %s' %
          ('loaded' if inodePathInfo.loaded else 'unloaded',
           os.path.normpath(os.path.join(mount, inodePathInfo.path)) if
               inodePathInfo.linked else 'unlinked'))


def get_loaded_inode_count(inode_info):
    count = 0
    for tree in inode_info:
        for inode in tree.entries:
            if inode.loaded:
                count += 1
    return count


def do_unload_inodes(args: argparse.Namespace):
    config = cmd_util.create_config(args)
    mount, rel_path = get_mount_path(args.path)

    with config.get_thrift_client() as client:
        inodeInfo_before_unload = client.debugInodeStatus(mount, rel_path)
        inodeCount_before_unload = get_loaded_inode_count(
            inodeInfo_before_unload)

        # set the age in nanoSeconds
        age = TimeSpec()
        age.seconds = int(args.age)
        age.nanoSeconds = int((args.age - age.seconds) * 10**9)
        client.unloadInodeForPath(mount, rel_path, age)

        inodeInfo_after_unload = client.debugInodeStatus(mount, rel_path)
        inodeCount_after_unload = get_loaded_inode_count(inodeInfo_after_unload)
        count = inodeCount_before_unload - inodeCount_after_unload
        print('Unloaded %s Inodes under the directory : %s' %
              (count, args.path))


def do_flush_cache(args: argparse.Namespace):
    config = cmd_util.create_config(args)
    mount, rel_path = get_mount_path(args.path)

    with config.get_thrift_client() as client:
        client.invalidateKernelInodeCache(mount, rel_path)


def do_set_log_level(args: argparse.Namespace):
    config = cmd_util.create_config(args)

    with config.get_thrift_client() as client:
        result = client.debugSetLogLevel(args.category, args.level)
        if result.categoryCreated:
            print("Warning: New category '{}' created. Did you mistype?".format(
                args.category))


class StdoutPrinter:
    def __init__(self):
        if sys.stdout.isatty():
            import curses
            curses.setupterm()
            self._bold = (curses.tigetstr('bold') or b'').decode()
            set_foreground = curses.tigetstr('setaf') or b''
            self._red = curses.tparm(set_foreground, curses.COLOR_RED).decode()
            self._green = curses.tparm(set_foreground, curses.COLOR_GREEN).decode()
            self._reset = (curses.tigetstr('sgr0') or b'').decode()
        else:
            self._bold = ''
            self._red = ''
            self._green = ''
            self._reset = ''

    def bold(self, text: str) -> str:
        return self._bold + text + self._reset

    def green(self, text: str) -> str:
        return self._green + text + self._reset

    def red(self, text: str) -> str:
        return self._red + text + self._reset


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
        'buildinfo',
        help='Show the build info for the Eden server')
    parser.set_defaults(func=do_buildinfo)

    parser = subparsers.add_parser(
        'hg_copy_map_get_all', help='Copymap for dirstate')
    parser.add_argument(
        'path', nargs='?', default=os.getcwd(),
        help='The path to an Eden mount point. Uses `pwd` by default.')
    parser.set_defaults(func=do_hg_copy_map_get_all)

    parser = subparsers.add_parser(
        'hg_dirstate', help='Print full dirstate')
    parser.add_argument(
        'path', nargs='?', default=os.getcwd(),
        help='The path to an Eden mount point. Uses `pwd` by default.')
    parser.set_defaults(func=do_hg_dirstate)

    parser = subparsers.add_parser(
        'hg_get_dirstate_tuple', help='Dirstate status for file')
    parser.add_argument(
        'path',
        help='The path to the file whose status should be queried.')
    parser.set_defaults(func=do_hg_get_dirstate_tuple)

    parser = subparsers.add_parser(
        'inode', help='Show data about loaded inodes')
    parser.add_argument(
        'path',
        help='The path to the eden mount point.  If a subdirectory inside '
        'a mount point is specified, only data about inodes under the '
        'specified subdirectory will be reported.')
    parser.set_defaults(func=do_inode)

    parser = subparsers.add_parser(
        'overlay', help='Show data about the overlay')
    parser.add_argument(
        '-n', '--number',
        type=int,
        help='Display information for the specified inode number.')
    parser.add_argument(
        '-d', '--depth',
        type=int, default=0,
        help='Recurse to the specified depth.')
    parser.add_argument(
        '-r', '--recurse',
        action='store_const', const=-1, dest='depth', default=0,
        help='Recursively print child entries.')
    parser.add_argument(
        'path', nargs='?',
        help='The path to the eden mount point.')
    parser.set_defaults(func=do_overlay)

    parser = subparsers.add_parser(
        'getpath', help='Get the eden path that corresponds to an inode number')
    parser.add_argument(
        'path', nargs='?',
        help='The path to an Eden mount point. Uses `pwd` by default.')
    parser.add_argument(
        'number',
        type=int,
        help='Display information for the specified inode number.')
    parser.set_defaults(func=do_getpath)

    parser = subparsers.add_parser(
        'unload', help='Unload unused inodes')
    parser.add_argument(
        'path',
        help='The path to the eden mount point.  If a subdirectory inside '
        'a mount point is specified, only inodes under the '
        'specified subdirectory will be unloaded.')
    parser.add_argument(
        'age',
        type=float,
        help='Minimum age of the inodes to be unloaded in seconds'
    )
    parser.set_defaults(func=do_unload_inodes)

    parser = subparsers.add_parser(
        'flush_cache', help='Flush kernel cache for inode')
    parser.add_argument(
        'path',
        help='Path to a directory/file inside an eden mount.')
    parser.set_defaults(func=do_flush_cache)

    parser = subparsers.add_parser(
        'set_log_level',
        help='Set the log level for a given category in the edenfs daemon.')
    parser.add_argument(
        'category',
        type=str,
        help='Period-separated log category.')
    parser.add_argument(
        'level',
        type=str,
        help='Log level string as understood by stringToLogLevel.'
    )
    parser.set_defaults(func=do_set_log_level)

    parser = subparsers.add_parser(
        'uptime',
        help='Check how long edenfs has been running.')
    parser.set_defaults(func=do_uptime)
