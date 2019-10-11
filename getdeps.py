#!/usr/bin/env python
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import, division, print_function, unicode_literals

import argparse
import os
import shlex
import shutil
import subprocess
import sys


try:
    from shlex import quote as shellquote
except ImportError:
    from pipes import quote as shellquote


class BuildOptions(object):
    def __init__(self, num_jobs, external_dir, install_dir):
        self.num_jobs = num_jobs
        if not self.num_jobs:
            import multiprocessing

            self.num_jobs = multiprocessing.cpu_count()

        self.external_dir = external_dir
        self.install_dir = install_dir

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

    def ensure_checkedout(self):
        self.updater.ensure_checkedout(self)

    def build(self):
        self.builder.build(self)

    def clean(self):
        self.updater.clean(self)


class GitUpdater(object):
    def __init__(self, repo, branch="master"):
        self.origin_repo = repo
        self.branch = branch

    def ensure_checkedout(self, project):
        if not os.path.exists(project.path):
            self._checkout(project)

    def update(self, project):
        if os.path.exists(project.path):
            print("Updating %s..." % project.name)
            run_cmd(["git", "-C", project.path, "fetch", "origin"])
            run_cmd(
                [
                    "git",
                    "-C",
                    project.path,
                    "merge",
                    "--ff-only",
                    "origin/%s" % self.branch,
                ]
            )
        else:
            self._checkout(project)

    def _checkout(self, project):
        print("Cloning %s..." % project.name)
        run_cmd(
            ["git", "clone", self.origin_repo, project.path, "--branch", self.branch]
        )
        run_cmd(["git", "submodule", "update", "--init"], cwd=project.path)

    def clean(self, project):
        run_cmd(["git", "-C", project.path, "clean", "-fxd"])


def fixup_env_for_darwin(env):
    def add_flag(name, item, separator, append=True):
        val = env.get(name, "").split(separator)
        if append:
            val.append(item)
        else:
            val.insert(0, item)
        env[name] = separator.join(val)

    # The brew/openssl installation situation is a bit too weird for vanilla
    # cmake logic to find, and most packages don't deal with this correctly,
    # so inject these into the environment to give them a hand
    add_flag("PKG_CONFIG_PATH", "/usr/local/opt/openssl/lib/pkgconfig", ":")
    add_flag("LDFLAGS", "-L/usr/local/opt/openssl/lib", " ")
    add_flag("CPPFLAGS", "-I/usr/local/opt/openssl/include", " ")

    # system bison is ancient, so ensure that the brew installed one takes
    # precedence.  Brew refuses to to install or link bison into /usr/local/bin,
    # so we have to insert this opt path instead.  Likewise for flex.
    add_flag("PATH", "/usr/local/opt/bison/bin", ":", append=False)
    add_flag("PATH", "/usr/local/opt/flex/bin", ":", append=False)

    # flex generates code that sprinkles the `register` keyword liberally
    # and the thrift compilation flags hate that in C++17 code.  Disable
    # the warning that promotes this to an error.
    add_flag("CXXFLAGS", "-Wno-register", " ")


class BuilderBase(object):
    def __init__(self, subdir=None, env=None, build_dir=None):
        self.env = os.environ.copy()
        if sys.platform == "darwin":
            fixup_env_for_darwin(self.env)
        if env:
            self.env.update(env)

        self.subdir = subdir
        self.build_dir = build_dir
        self._build_path = None

    def _run_cmd(self, cmd):
        run_cmd(cmd=cmd, env=self.env, cwd=self._build_path)

    def build(self, project):
        print("Building %s..." % project.name)
        if self.subdir:
            build_path = os.path.join(project.path, self.subdir)
        else:
            build_path = project.path

        if self.build_dir is not None:
            build_path = os.path.join(build_path, self.build_dir)
            if not os.path.isdir(build_path):
                os.mkdir(build_path)

        self._build_path = build_path
        try:
            self._build(project)
        finally:
            self._build_path = None


class NullBuilder(BuilderBase):
    def __init__(self):
        super(NullBuilder, self).__init__()

    def _build(self, project):
        pass


class MakeBuilder(BuilderBase):
    def __init__(self, subdir=None, env=None, args=None):
        super(MakeBuilder, self).__init__(subdir=subdir, env=env)
        self.args = args or []

    def _build(self, project):
        cmd = ["make", "-j%s" % project.opts.num_jobs] + self.args
        self._run_cmd(cmd)

        install_cmd = ["make", "install", "PREFIX=" + project.opts.install_dir]
        self._run_cmd(install_cmd)


class AutoconfBuilder(BuilderBase):
    def __init__(self, subdir=None, env=None, args=None):
        super(AutoconfBuilder, self).__init__(subdir=subdir, env=env)
        self.args = args or []

    def _build(self, project):
        configure_path = os.path.join(self._build_path, "configure")
        if not os.path.exists(configure_path):
            self._run_cmd(["autoreconf", "--install"])
        configure_cmd = [
            configure_path,
            "--prefix=" + project.opts.install_dir,
        ] + self.args
        self._run_cmd(configure_cmd)
        self._run_cmd(["make", "-j%s" % project.opts.num_jobs])
        self._run_cmd(["make", "install"])


class CMakeBuilder(BuilderBase):
    def __init__(self, subdir=None, env=None, defines=None):
        super(CMakeBuilder, self).__init__(subdir=subdir, env=env, build_dir="_build")
        self.defines = defines or {}

    def _build(self, project):
        defines = {
            "CMAKE_INSTALL_PREFIX": project.opts.install_dir,
            "BUILD_SHARED_LIBS": "OFF",
        }
        defines.update(self.defines)
        define_args = ["-D%s=%s" % (k, v) for (k, v) in defines.items()]

        self._run_cmd(["cmake", "configure", ".."] + define_args)
        self._run_cmd(["make", "-j%s" % project.opts.num_jobs])
        self._run_cmd(["make", "install"])


def run_cmd(cmd, env=None, cwd=None):
    cmd_str = " ".join(shellquote(arg) for arg in cmd)
    print("+ " + cmd_str)
    subprocess.check_call(cmd, env=env, cwd=cwd)


def install_apt(pkgs):
    cmd = ["sudo", "apt-get", "install", "-yq"] + pkgs
    run_cmd(cmd)


def get_projects(opts):
    projects = [
        Project(
            "mstch",
            opts,
            GitUpdater("https://github.com/no1msd/mstch.git"),
            CMakeBuilder(),
        ),
        Project(
            "cpptoml",
            opts,
            GitUpdater("https://github.com/skystrife/cpptoml.git"),
            CMakeBuilder(),
        ),
        Project(
            "rocksdb",
            opts,
            GitUpdater("https://github.com/facebook/rocksdb.git"),
            CMakeBuilder(defines={"WITH_SNAPPY": "ON", "WITH_TESTS": "OFF"}),
        ),
        Project(
            "googletest",
            opts,
            GitUpdater("https://github.com/google/googletest.git"),
            CMakeBuilder(),
        ),
        Project(
            "folly",
            opts,
            GitUpdater("https://github.com/facebook/folly.git"),
            CMakeBuilder(defines={"BUILD_TESTS": "OFF"}),
        ),
        Project(
            "libsodium",
            opts,
            GitUpdater("https://github.com/jedisct1/libsodium.git"),
            AutoconfBuilder(),
        ),
        Project(
            "fizz",
            opts,
            GitUpdater("https://github.com/facebookincubator/fizz.git"),
            CMakeBuilder(
                subdir="fizz", defines={"BUILD_EXAMPLES": "OFF", "BUILD_TESTS": "OFF"}
            ),
        ),
        Project(
            "wangle",
            opts,
            GitUpdater("https://github.com/facebook/wangle.git"),
            CMakeBuilder(subdir="wangle", defines={"BUILD_TESTS": "OFF"}),
        ),
        Project(
            "rsocket-cpp",
            opts,
            GitUpdater("https://github.com/rsocket/rsocket-cpp.git"),
            CMakeBuilder(
                defines={
                    "BUILD_EXAMPLES": "OFF",
                    "BUILD_BENCHMARKS": "OFF",
                    "BUILD_TESTS": "OFF",
                }
            ),
        ),
        Project(
            "fbthrift",
            opts,
            GitUpdater("https://github.com/facebook/fbthrift.git"),
            CMakeBuilder(),
        ),
    ]
    if sys.platform == "darwin":
        projects.append(
            Project(
                "osxfuse",
                opts,
                GitUpdater(
                    "https://github.com/osxfuse/osxfuse.git", branch="support/osxfuse-3"
                ),
                NullBuilder(),
            )
        )

    return projects


def get_linux_type():
    try:
        with open("/etc/os-release") as f:
            data = f.read()
    except EnvironmentError:
        return (None, None)

    os_vars = {}
    for line in data.splitlines():
        parts = line.split("=", 1)
        if len(parts) != 2:
            continue
        key = parts[0].strip()
        value_parts = shlex.split(parts[1].strip())
        if not value_parts:
            value = ""
        else:
            value = value_parts[0]
        os_vars[key] = value

    return os_vars.get("NAME"), os_vars.get("VERSION_ID")


def get_os_type():
    if sys.platform.startswith("linux"):
        return get_linux_type()
    elif sys.platform.startswith("darwin"):
        return ("darwin", None)
    elif sys.platform == "windows":
        return ("windows", sys.getwindowsversion().major)
    else:
        return (None, None)


def install_platform_deps():
    os_name, os_version = get_os_type()
    if os_name is None:
        raise Exception("unable to detect OS type")
    elif os_name == "Ubuntu":
        # These dependencies have been tested on Ubuntu 16.04
        print("Installing necessary Ubuntu packages...")
        ubuntu_pkgs = (
            "autoconf automake libdouble-conversion-dev "
            "libssl-dev make zip git libtool g++ libboost-all-dev "
            "libevent-dev flex bison libgoogle-glog-dev libkrb5-dev "
            "libsnappy-dev libsasl2-dev libnuma-dev libcurl4-gnutls-dev "
            "libpcap-dev libdb5.3-dev cmake libfuse-dev libgit2-dev mercurial "
            "libsqlite3-dev libzstd-dev"
        ).split()
        install_apt(ubuntu_pkgs)
    elif os_name == "darwin":
        print("Installing necessary packages via Homebrew...")
        run_cmd(
            [
                "brew",
                "install",
                "autoconf",
                "automake",
                "bison",
                "boost",
                "boost-python",
                "cmake",
                "curl",
                "double-conversion",
                "flex",
                "gflags",
                "glog",
                "libevent",
                "libgit2",
                "libtool",
                "lz4",
                "openssl",
                "snappy",
                "xz",
                "zstd",
            ]
        )
    else:
        # TODO: Handle distributions other than Ubuntu.
        raise Exception(
            "installing OS dependencies on %s is not supported yet" % (os_name,)
        )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "-o",
        "--external-dir",
        help="The directory where external projects should be "
        'created (default="external")',
    )
    ap.add_argument(
        "-u",
        "--update",
        action="store_true",
        default=False,
        help="Updates the external projects repositories before building them",
    )
    ap.add_argument(
        "-C",
        "--clean",
        action="store_true",
        default=None,
        help="Cleans the external project repositories before "
        "building them (defaults to on when updating projects)",
    )
    ap.add_argument(
        "--no-clean",
        action="store_false",
        default=None,
        dest="clean",
        help="Do not clean the external project repositories "
        "even after updating them.",
    )
    ap.add_argument(
        "-j",
        "--jobs",
        dest="num_jobs",
        type=int,
        default=None,
        help="The number of jobs to run in parallel when building",
    )
    ap.add_argument(
        "--install-dir",
        help="Directory where external projects should be "
        "installed (default=<external-dir>/install)",
    )
    ap.add_argument(
        "--system-deps",
        action="store_true",
        default=False,
        help="Also install necessary packages available from OS's package "
        "distribution system",
    )

    args = ap.parse_args()

    if args.external_dir is None:
        script_dir = os.path.abspath(os.path.dirname(__file__))
        args.external_dir = os.path.join(script_dir, "external")
    if args.install_dir is None:
        args.install_dir = os.path.join(args.external_dir, "install")
    if args.clean is None:
        args.clean = args.update

    opts = BuildOptions(args.num_jobs, args.external_dir, args.install_dir)

    if args.system_deps:
        install_platform_deps()

    if not os.path.isdir(opts.external_dir):
        os.makedirs(opts.external_dir)

    projects = get_projects(opts)
    for project in projects:
        if args.update:
            project.update()
        else:
            project.ensure_checkedout()

    if args.clean:
        shutil.rmtree(args.install_dir)
        for project in projects:
            project.clean()

    for project in projects:
        project.build()


if __name__ == "__main__":
    main()
