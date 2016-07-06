#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import argparse
from eden.thrift import EdenNotRunningError
import errno
import json
import os
import subprocess
import sys
import thrift

from . import config as config_mod
from . import util
from fb303.ttypes import fb_status

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
    '''
    If path points to a git repository, return the path to the repository .git
    directory.  Otherwise, if the path is not a git repository, return None.
    '''
    path = os.path.realpath(path)
    if path.endswith('.git') and _is_git_dir(path):
        return path

    git_subdir = os.path.join(path, '.git')
    if _is_git_dir(git_subdir):
        return git_subdir

    return None


def _get_git_commit(git_dir):
    cmd = ['git', '--git-dir', git_dir, 'rev-parse', 'HEAD']
    out = subprocess.check_output(cmd)
    return out.strip().decode('utf-8', errors='surrogateescape')


def _get_hg_repo(path):
    '''
    If path points to a mercurial repository, return a normalized path to the
    repository root.  Otherwise, if path is not a mercurial repository, return
    None.
    '''
    repo_path = os.path.realpath(path)
    hg_dir = os.path.join(repo_path, '.hg')
    if not os.path.isdir(hg_dir):
        return None

    # Check to see if this is a shared working directory from another
    # repository
    try:
        with open(os.path.join(hg_dir, 'sharedpath'), 'r') as f:
            hg_dir = f.readline().rstrip('\n')
            hg_dir = os.path.realpath(hg_dir)
            repo_path = os.path.dirname(hg_dir)
    except EnvironmentError as ex:
        if ex.errno != errno.ENOENT:
            raise

    if not os.path.isdir(os.path.join(hg_dir, 'store')):
        return None

    return repo_path


def _get_hg_commit(repo):
    env = os.environ.copy()
    env['HGPLAIN'] = '1'
    cmd = ['hg', '--cwd', repo, 'log', '-T{node}\\n', '-r.']
    out = subprocess.check_output(cmd, env=env)
    return out.strip().decode('utf-8', errors='surrogateescape')


def do_health(args):
    config = create_config(args)
    health_info = config.check_health()
    if health_info.is_healthy():
        print('eden running normally (pid {})'.format(health_info.pid))
        return 0

    print('edenfs not healthy: {}'.format(health_info.detail))
    return 1


def do_repository(args):
    config = create_config(args)
    if (args.name and args.path):
        repo_type = None
        git_dir = _get_git_dir(args.path)
        if git_dir:
            repo_type = 'git'
            source = git_dir
        else:
            hg_repo = _get_hg_repo(args.path)
            if hg_repo:
                repo_type = 'hg'
                source = hg_repo
        if repo_type is None:
            print_stderr(
                '%s does not look like a git or hg repository' % args.path)
            return 1
        try:
            config.add_repository(args.name,
                                  repo_type=repo_type,
                                  source=source,
                                  with_buck=args.with_buck)
        except Exception as ex:
            print_stderr('{}', ex)
            return 1
    elif (args.name or args.path):
        print_stderr('repository command called with incorrect arguments')
        return 1
    else:
        try:
            repo_list = config.get_repository_list()
        except Exception as ex:
            print_stderr('error: {}', ex)
            return 1
        for repo in repo_list:
            print(repo)


def do_list(args):
    config = create_config(args)
    for name in config.get_client_names():
        print(name)


def do_clone(args):
    args.path = normalize_path_arg(args.path)
    config = create_config(args)
    snapshot_id = args.snapshot
    if not snapshot_id:
        try:
            source = config.get_repo_source(args.repo)
        except Exception as ex:
            print_stderr('{}', ex)
            return 1

        if source['type'] == 'git':
            snapshot_id = _get_git_commit(source['path'])
        elif source['type'] == 'hg':
            snapshot_id = _get_hg_commit(source['path'])
        else:
            print_stderr(
                '%s does not look like a git or hg repository' % args.path)
            return 1
    try:
        return config.clone(args.repo, args.path, snapshot_id)
    except Exception as ex:
        print_stderr('error: {}', ex)
        return 1


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
    args.path = normalize_path_arg(args.path)
    config = create_config(args)
    try:
        return config.unmount(args.path)
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
    daemon_binary = args.daemon_binary or _find_default_daemon_binary()

    # If this is the first time running the daemon, the ~/.eden directory
    # structure needs to be set up.
    # TODO(mbolin): Check whether the user is running as sudo/root. In general,
    # we want to avoid creating ~/.eden as root.
    _ensure_dot_eden_folder_exists(config)

    # If the user put an "--" argument before the edenfs args, argparse passes
    # that through to us.  Strip it out.
    edenfs_args = args.edenfs_args
    if edenfs_args and edenfs_args[0] == '--':
        edenfs_args = edenfs_args[1:]

    try:
        health_info = config.spawn(daemon_binary, edenfs_args, gdb=args.gdb,
                                   foreground=args.foreground)
    except config_mod.EdenStartError as ex:
        print_stderr('error: {}', ex)
        return 1
    print('Started edenfs (pid {}). Logs available at {}'.format(
        health_info.pid, config.get_log_path()))
    return 0


def _find_default_daemon_binary():
    # By default, we look for the daemon executable alongside this file.
    script_dir = os.path.dirname(os.path.abspath(sys.argv[0]))
    candidate = os.path.join(script_dir, 'edenfs')
    permissions = os.R_OK | os.X_OK
    if os.access(candidate, permissions):
        return candidate

    # This is where the binary will be found relative to this file when it is
    # run out of buck-out in debug mode.
    candidate = os.path.normpath(os.path.join(script_dir, '../service/edenfs'))
    if os.access(candidate, permissions):
        return candidate
    else:
        return None


def _ensure_dot_eden_folder_exists(config):
    '''Creates the ~/.eden folder as specified by --config-dir/$EDEN_CONFIG_DIR.
    If the ~/.eden folder already exists, it will be left alone.

    Returns the path to the RocksDB.
    '''
    db = config.get_or_create_path_to_rocks_db()
    return db


def do_shutdown(args):
    config = create_config(args)
    client = None
    try:
        client = config.get_thrift_client()
        pid = client.getPid()
    except EdenNotRunningError:
        print_stderr('error: edenfs is not running')
        return 1

    # Ask the client to shutdown
    client.shutdown()

    if args.timeout == 0:
        print('Sent shutdown request to edenfs.')
        return 0

    # Wait until the process exits.
    def eden_exited():
        try:
            os.kill(pid, 0)
        except OSError as ex:
            if ex.errno == errno.ESRCH:
                # The process has exited
                return True
            # EPERM is okay (and means the process is still running),
            # anything else is unexpected
            if ex.errno != errno.EPERM:
                raise
        # Still running
        return None

    try:
        util.poll_until(eden_exited, timeout=args.timeout)
        print('edenfs exited')
        return 0
    except util.TimeoutError:
        print_stderr('error: sent shutdown request, but edenfs did not exit '
                     'within {} seconds', args.timeout)
        return 1


def create_parser():
    '''Returns a parser and its immediate subparsers.'''
    parser = argparse.ArgumentParser(description='Manage Eden clients.')
    parser.add_argument(
        '--config-dir',
        help='Path to directory where client data is stored.')
    parser.add_argument(
        '--home-dir',
        help='Path to directory where .edenrc config file is stored.')
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

    clone_parser = subparsers.add_parser(
        'clone', help='Create a clone of a specific repo')
    clone_parser.add_argument(
        'repo', help='Name of repository to clone')
    clone_parser.add_argument(
        'path', help='Path where the client should be mounted')
    clone_parser.add_argument(
        '--snapshot', '-s', type=str, help='Snapshot id of revision')
    clone_parser.set_defaults(func=do_clone)

    daemon_parser = subparsers.add_parser(
        'daemon', help='Run the edenfs daemon')
    daemon_parser.add_argument(
        '--daemon-binary',
        help='Path to the binary for the Eden daemon.')
    daemon_parser.add_argument(
        '--foreground', '-F', action='store_true',
        help='Run eden in the foreground, rather than daemonizing')
    daemon_parser.add_argument(
        '--gdb', '-g', action='store_true', help='Run under gdb')
    daemon_parser.add_argument(
        'edenfs_args', nargs=argparse.REMAINDER,
        help='Any extra arguments after an "--" argument will be passed to the '
        'edenfs daemon.')
    daemon_parser.set_defaults(func=do_daemon)

    health_parser = subparsers.add_parser(
        'health', help='Check the health of the Eden service')
    health_parser.set_defaults(func=do_health)

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

    list_parser = subparsers.add_parser(
        'list', help='List available clients')
    list_parser.set_defaults(func=do_list)

    repository_parser = subparsers.add_parser(
        'repository', help='List all repositories')
    repository_parser.add_argument(
        'name', nargs='?', default=None, help='Name of the client to mount')
    repository_parser.add_argument(
        'path',
        nargs='?',
        default=None,
        help='Path to the repository to import')
    repository_parser.add_argument(
        '--with-buck', '-b', action='store_true',
        help='Client should create a bind mount for buck-out/.')
    repository_parser.set_defaults(func=do_repository)

    shutdown_parser = subparsers.add_parser(
        'shutdown', help='Shutdown the daemon')
    shutdown_parser.add_argument(
        '-t', '--timeout', type=float, default=15.0,
        help='Wait up to TIMEOUT seconds for the daemon to exit.  '
        '(default=%(default)s).  If timeout is 0, then do not wait at all.')
    shutdown_parser.set_defaults(func=do_shutdown)

    unmount_parser = subparsers.add_parser(
        'unmount', help='Unmount a specific client')
    unmount_parser.add_argument(
        'path', help='Path where client should be unmounted from')
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

    home_dir = util.get_home_dir()
    return os.path.join(home_dir, DEFAULT_CONFIG_DIR)


def create_config(args):
    config = args.config_dir or find_default_config_dir()
    home_dir = args.home_dir or util.get_home_dir()
    return config_mod.Config(config, home_dir)


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
