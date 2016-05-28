# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import (absolute_import, division,
                        print_function, unicode_literals)

import argparse
from eden.thrift import EdenNotRunningError
import errno
import glue
import json
import os
import subprocess
import sys

from . import config as config_mod

# Relative to the user's $HOME/%USERPROFILE% directory.
# TODO: This value should be .eden outside of Facebook devservers.
DEFAULT_CONFIG_DIR = 'local/.eden'

# Environment variable that can be used instead of specifying --config-dir.
CONFIG_DIR_ENVIRONMENT_VARIABLE = 'EDEN_CONFIG_DIR'

def infer_client_from_cwd(config, clientname):
    if clientname:
        return clientname

    all_clients = config.get_all_client_config_info()
    path = normalize_path_arg(os.getcwd())

    # Keep going while we're not in the root, as dirname(/) is /
    # and we can keep iterating forever.
    while len(path) > 1:
        for client, info in all_clients.iteritems():
            if info['mount'] == path:
                return client
        path = os.path.dirname(path)

    print_stderr(
        'cwd is not an eden mount point, and no client name was specified.')
    sys.exit(2)


def do_help(args, parser, subparsers):
    help_args = args.args
    num_help_args = len(help_args)
    if num_help_args == 1:
        name = args.args[0]
        subparser = subparsers.choices.get(name, None)
        if subparser:
            subparser.parse_args(['--help'])
        else:
            print_stderr('No manual entry for %s' % name)
            sys.exit(2)
    elif num_help_args == 0:
        parser.parse_args(['--help'])
    else:
        print_stderr('Too many args passed to help: %s' % help_args)
        sys.exit(2)


def do_info(args):
    config = create_config(args)
    info = config.get_client_info(infer_client_from_cwd(config, args.client))
    json.dump(info, sys.stdout, indent=2)
    sys.stdout.write('\n')


def _is_git_dir(path):
    return (os.path.isdir(os.path.join(path, 'objects')) and
            os.path.isdir(os.path.join(path, 'refs')) and
            os.path.exists(os.path.join(path, 'HEAD')))


def _get_git_dir(path):
    path = os.path.realpath(path)
    if path.endswith('.git') and _is_git_dir(path):
        return path

    git_subdir = os.path.join(path, '.git')
    if _is_git_dir(git_subdir):
        return git_subdir

    return None


def _get_hg_dir(path):
    hg_dir = os.path.join(os.path.realpath(path), '.hg')
    if not os.path.exists(os.path.join(hg_dir, 'hgrc')):
        return None

    # Check to see if this is a shared working directory from another
    # repository
    try:
        with open(os.path.join(hg_dir, 'sharedpath'), 'r') as f:
            hg_dir = f.readline().rstrip('\n')
    except EnvironmentError as ex:
        if ex.errno != errno.ENOENT:
            raise
        return None

    if not os.path.isdir(os.path.join(hg_dir, 'store')):
        return None

    return hg_dir


def _get_hg_commit(repo):
    env = os.environ.copy()
    env['HGPLAIN'] = '1'
    cmd = ['hg', '--cwd', repo, 'log', '-T{node}\\n', '-r.']
    out = subprocess.check_output(cmd, env=env)
    return out.strip()


def do_init(args):
    args.mount = normalize_path_arg(args.mount)
    args.repo = normalize_path_arg(args.repo)

    config = create_config(args)
    db = config.get_or_create_path_to_rocks_db()

    # Check to see if we can figure out the repository type
    snapshot_id = None
    git_dir = _get_git_dir(args.repo)
    if git_dir is not None:
        snapshot_id = glue.do_git_import(git_dir, db)
        config.create_client(args.name, args.mount, snapshot_id,
                             repo_type='git',
                             repo_source=git_dir,
                             with_buck=args.with_buck)
        return

    hg_dir = _get_hg_dir(args.repo)
    if hg_dir is not None:
        snapshot_id = _get_hg_commit(args.repo)
        config.create_client(args.name, args.mount, snapshot_id,
                             repo_type='hg',
                             repo_source=hg_dir,
                             with_buck=args.with_buck)
        return

    raise Exception('{} does not look like a git or hg repository'.format(
        args.repo))


def do_list(args):
    config = create_config(args)
    for name in config.get_client_names():
        print(name)


def do_mount(args):
    config = create_config(args)
    try:
        return config.mount(args.name)
    except EdenNotRunningError as ex:
        # TODO: Eventually it would be nice to automatically start the edenfs
        # daemon for the user, and run it in the background.
        print_stderr('error: {}', ex)
        progname = os.path.basename(sys.argv[0])
        print_stderr('Try starting edenfs first with "{} daemon"', progname)
        return 1


def do_unmount(args):
    config = create_config(args)
    try:
        return config.unmount(args.name)
    except Exception as ex:
        print_stderr('error: {}', ex)
        return 1

def do_checkout(args):
    config = create_config(args)
    try:
        config.checkout(infer_client_from_cwd(config, args.client),
                        args.snapshot)
    except Exception as ex:
        print_stderr('checkout of %s failed for client %s: %s' % (
                     args.snapshot,
                     args.client,
                     str(ex)))
        sys.exit(1)


def do_daemon(args):
    config = create_config(args)
    return config.spawn(debug=args.debug, gdb=args.gdb)


def create_parser():
    '''Returns a parser and its immediate subparsers.'''
    parser = argparse.ArgumentParser(description='Manage Eden clients.')
    parser.add_argument(
        '--config-dir',
        help='Path to directory where client data is stored.',
        default=find_default_config_dir())
    subparsers = parser.add_subparsers(dest='subparser_name')

    # Please add the subparsers in alphabetical order because that is the order
    # in which they are displayed when the user runs --help.
    checkout_parser = subparsers.add_parser(
        'checkout', help='Check out an alternative snapshot hash.')
    checkout_parser.add_argument('--client', '-c',
                                 default=None,
                                 help='Name of the mounted client')
    checkout_parser.add_argument('snapshot', help='Snapshot hash to check out')
    checkout_parser.set_defaults(func=do_checkout)

    help_parser = subparsers.add_parser(
        'help', help='Display help information about Eden.')
    help_parser.set_defaults(func=do_help)
    help_parser.add_argument('args', nargs='*')

    info_parser = subparsers.add_parser(
        'info', help='Get details about a client.')
    info_parser.add_argument(
        'client',
        default=None,
        nargs='?',
        help='Name of the client')
    info_parser.set_defaults(func=do_info)

    init_parser = subparsers.add_parser(
        'init', help='Create a new Eden client.')
    init_parser.add_argument(
        '--repo', help='Path to the repository to import.',
        required=True)
    init_parser.add_argument(
        '--mount', '-m', help='Path where the client should be mounted.',
        required=True)
    init_parser.add_argument(
        '--with-buck', '-b', action='store_true',
        help='Client should create a bind mount for buck-out/.')
    init_parser.add_argument(
        'name', help='Name of the new client')
    init_parser.set_defaults(func=do_init)

    list_parser = subparsers.add_parser(
        'list', help='List available clients')
    list_parser.set_defaults(func=do_list)

    mount_parser = subparsers.add_parser(
        'mount', help='Mount a specific client')
    mount_parser.add_argument(
        'name', help='Name of the client to mount')
    mount_parser.set_defaults(func=do_mount)

    daemon_parser = subparsers.add_parser(
        'daemon', help='Run the edenfs daemon')
    daemon_parser.add_argument(
        '--gdb', '-g', action='store_true', help='Run under gdb')
    daemon_parser.add_argument(
        '--debug', '-d', action='store_true', help='Enable fuse debugging.')
    daemon_parser.set_defaults(func=do_daemon)

    unmount_parser = subparsers.add_parser(
        'unmount', help='Unmount a specific client')
    unmount_parser.add_argument(
        'name', help='Name of client to unmount')
    unmount_parser.set_defaults(func=do_unmount)

    return parser, subparsers


def find_default_config_dir():
    '''Returns the path to default Eden config directory.

    If the environment variable $EDEN_CONFIG_DIR is set, it takes precedence
    over the default, which is "$HOME/.eden".

    Note that the path is not guaranteed to correspond to an existing directory.
    '''
    config_dir = os.getenv(CONFIG_DIR_ENVIRONMENT_VARIABLE)
    if config_dir:
        return config_dir

    if os.name == 'nt':
        home_dir = os.getenv('USERPROFILE')
    else:
        home_dir = os.getenv('HOME')
    return os.path.join(home_dir, DEFAULT_CONFIG_DIR)


def create_config(args):
    return config_mod.Config(args.config_dir)


def main():
    parser, subparsers = create_parser()
    args = parser.parse_args()
    if args.subparser_name == 'help':
        retcode = args.func(args, parser, subparsers)
    else:
        retcode = args.func(args)
    return retcode


def print_stderr(message, *args, **kwargs):
    '''Prints the message to stderr.'''
    if args or kwargs:
        message = message.format(*args, **kwargs)
    print(message, file=sys.stderr)


def normalize_path_arg(path_arg):
    '''
    This ensures that path expansions such as ~ are handled properly and that
    relative paths are made absolute.
    '''
    if path_arg:
        return os.path.abspath(os.path.normpath(os.path.expanduser(path_arg)))
    else:
        return path_arg


if __name__ == '__main__':
    retcode = main()
    sys.exit(retcode)
