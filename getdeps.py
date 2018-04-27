#!/usr/bin/env python
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals

import argparse
import os
import shlex
import subprocess
import sys

try:
    from shlex import quote as shellquote
except ImportError:
    from pipes import quote as shellquote


class BuildOptions(object):
    def __init__(self, external_dir, num_jobs=None):
        self.external_dir = external_dir
        self.update = False
        self.num_jobs = num_jobs
        if not self.num_jobs:
            import multiprocessing
            self.num_jobs = multiprocessing.cpu_count()

    def project_dir(self, name, *paths):
        return os.path.join(self.external_dir, name, *paths)


class Project(object):
    def __init__(self, name, opts, updater, builder):
        self.name = name
        self.opts = opts
        self.updater = updater
        self.builder = builder
        self.path = self.opts.project_dir(self.name)

    def update(self):
        self.updater.update(self)

    def build(self):
        self.builder.build(self)


class GitUpdater(object):
    def __init__(self, repo, branch='master'):
        self.origin_repo = repo
        self.branch = branch

    def update(self, project):
        if os.path.exists(project.path):
            if not project.opts.update:
                return
            print('Updating %s...' % project.name)
            run_cmd(['git', '-C', project.path, 'fetch', 'origin'])
            run_cmd(['git', '-C', project.path, 'merge', '--ff-only',
                     'origin/%s' % self.branch])
        else:
            print('Cloning %s...' % project.name)
            run_cmd(['git', 'clone', self.origin_repo, project.path,
                     '--branch', self.branch])


class MakeBuilder(object):
    def __init__(self, subdir=None, env=None, args=None):
        self.subdir = subdir
        self.env = env
        self.args = args
        self.is_autoconf = False

    def build(self, project):
        print('Building %s...' % project.name)
        if self.env:
            env = os.environ.copy()
            env.update(self.env)
        else:
            env = None

        if self.subdir:
            build_path = os.path.join(project.path, self.subdir)
        else:
            build_path = project.path

        if self.is_autoconf:
            configure_path = os.path.join(build_path, 'configure')
            if not os.path.exists(configure_path):
                run_cmd(['autoreconf', '--install'], env=env, cwd=build_path)
            run_cmd([os.path.join(build_path, 'configure')],
                    env=env, cwd=build_path)

        cmd = ['make', '-j%s' % project.opts.num_jobs]
        if self.args:
            cmd.extend(self.args)
        run_cmd(cmd, env=env, cwd=build_path)


class AutoconfBuilder(MakeBuilder):
    def __init__(self, subdir=None, env=None, args=None):
        super(AutoconfBuilder, self).__init__(subdir=subdir, env=env, args=args)
        self.is_autoconf = True


class CMakeBuilder(object):
    def __init__(self, subdir=None, env=None, defines=None):
        self.subdir = subdir
        self.env = env
        self.defines = defines

    def build(self, project):
        print('Building %s...' % project.name)

        if self.env:
            env = os.environ.copy()
            env.update(self.env)
        else:
            env = None

        if self.subdir:
            build_path = os.path.join(project.path, self.subdir, 'build')
        else:
            build_path = os.path.join(project.path, 'build')

        if not os.path.isdir(build_path):
            os.mkdir(build_path)

        cmd = ['cmake', '..']
        if self.defines:
            define_args = ['-D%s=%s' % (k, v)
                           for (k, v) in self.defines.items()]
            cmd.extend(define_args)
        run_cmd(cmd, env=env, cwd=build_path)

        run_cmd(['make'], env=env, cwd=build_path)


def run_cmd(cmd, env=None, cwd=None):
    cmd_str = ' '.join(shellquote(arg) for arg in cmd)
    print('+ ' + cmd_str)
    subprocess.check_call(cmd, env=env, cwd=cwd)


def install_apt(pkgs):
    cmd = ['sudo', 'apt-get', 'install', '-yq'] + pkgs
    run_cmd(cmd)


def WangleBuilder(opts):
    cmake_defs = {
        'FOLLY_INCLUDE_DIR': opts.project_dir('folly'),
        'FOLLY_LIBRARY': opts.project_dir('folly', 'folly/.libs/libfolly.a'),
        'BUILD_TESTS': 'OFF',
    }
    return CMakeBuilder(subdir='wangle', defines=cmake_defs)


def FbthriftBuilder(opts):
    lib_dirs = [
        opts.project_dir('folly', 'folly/.libs'),
        opts.project_dir('wangle', 'wangle/build/lib'),
        opts.project_dir('mstch', 'build/src'),
        opts.project_dir('zstd', 'lib'),
    ]
    libdir_flags = [shellquote('-L' + path) for path in lib_dirs]
    cmake_env = {
        'FOLLY_INCLUDE_DIR': opts.project_dir('folly'),
        'MSTCH_INCLUDE_DIRS': opts.project_dir('mstch', 'include'),
        'WANGLE_INCLUDE_DIRS': opts.project_dir('wangle'),
        'ZSTD_INCLUDE_DIRS': opts.project_dir('zstd', 'lib'),
        'LDFLAGS': ' '.join(libdir_flags),
    }
    return CMakeBuilder(subdir='thrift', env=cmake_env)


def get_projects(opts):
    return [
        Project(
            'mstch', opts,
            GitUpdater('https://github.com/no1msd/mstch.git'),
            CMakeBuilder(),
        ),
        Project(
            'zstd', opts,
            GitUpdater('https://github.com/facebook/zstd.git'),
            MakeBuilder(),
        ),
        Project(
            'rocksdb', opts,
            GitUpdater('https://github.com/facebook/rocksdb.git'),
            MakeBuilder(args=['static_lib']),
        ),
        Project(
            'googletest', opts,
            GitUpdater('https://github.com/google/googletest.git'),
            CMakeBuilder(),
        ),
        Project(
            'folly', opts,
            GitUpdater('https://github.com/facebook/folly.git'),
            AutoconfBuilder(subdir='folly')
        ),
        Project(
            'wangle', opts,
            GitUpdater('https://github.com/facebook/wangle.git'),
            WangleBuilder(opts),
        ),
        Project(
            'fbthrift', opts,
            GitUpdater('https://github.com/facebook/fbthrift.git'),
            FbthriftBuilder(opts),
        ),
    ]


def get_linux_type():
    try:
        with open('/etc/os-release') as f:
            data = f.read()
    except EnvironmentError:
        return (None, None)

    os_vars = {}
    for line in data.splitlines():
        parts = line.split('=', 1)
        if len(parts) != 2:
            continue
        key = parts[0].strip()
        value_parts = shlex.split(parts[1].strip())
        if not value_parts:
            value = ''
        else:
            value = value_parts[0]
        os_vars[key] = value

    return os_vars.get('NAME'), os_vars.get('VERSION_ID')


def get_os_type():
    if sys.platform.startswith('linux'):
        return get_linux_type()
    elif sys.platform.startswith('darwin'):
        return ('darwin', None)
    elif sys.platform == 'windows':
        return ('windows', sys.getwindowsversion().major)
    else:
        return (None, None)


def install_platform_deps():
    os_name, os_version = get_os_type()
    if os_name is None:
        raise Exception('unable to detect OS type')
    elif os_name == 'Ubuntu':
        # These dependencies have been tested on Ubuntu 16.04
        print('Installing necessary Ubuntu packages...')
        ubuntu_pkgs = (
            'autoconf automake libdouble-conversion-dev '
            'libssl-dev make zip git libtool g++ libboost-all-dev '
            'libevent-dev flex bison libgoogle-glog-dev libkrb5-dev '
            'libsnappy-dev libsasl2-dev libnuma-dev libcurl4-gnutls-dev '
            'libpcap-dev libdb5.3-dev cmake libfuse-dev libgit2-dev mercurial '
        ).split()
        install_apt(ubuntu_pkgs)
    else:
        # TODO: Handle distributions other than Ubuntu.
        raise Exception('installing OS dependencies on %s is not '
                        'supported yet' % (os_name,))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('-o', '--external-dir',
                    help='The directory where external projects should be '
                    'created (default="external")')
    ap.add_argument('-u', '--update',
                    action='store_true',
                    default=False,
                    help='Updates the external projects repositories before '
                    'building them')
    ap.add_argument('-j', '--jobs',
                    dest='num_jobs',
                    type=int,
                    default=None,
                    help='The number of jobs to run in parallel when building')
    ap.add_argument('--install-deps',
                    action='store_true',
                    default=False,
                    help='Install necessary system packages')

    args = ap.parse_args()

    if args.external_dir is None:
        script_dir = os.path.abspath(os.path.dirname(__file__))
        args.external_dir = os.path.join(script_dir, 'external')
    opts = BuildOptions(args.external_dir, args.num_jobs)
    opts.update = args.update

    if args.install_deps:
        install_platform_deps()

    if not os.path.isdir(opts.external_dir):
        os.makedirs(opts.external_dir)

    projects = get_projects(opts)
    for project in projects:
        project.update()
    for project in projects:
        project.build()


if __name__ == '__main__':
    main()
