# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Infinitepush Bundle Store
"""store for infinitepush bundles"""

import hashlib
import os
import subprocess
from tempfile import NamedTemporaryFile

from edenscm.mercurial import error
from edenscm.mercurial.i18n import _


class bundlestore(object):
    def __init__(self, repo):
        storetype = repo.ui.config("infinitepush", "storetype", "")
        if storetype == "disk":
            self.store = filebundlestore(repo)
        elif storetype == "external":
            self.store = externalbundlestore(repo)
        else:
            raise error.Abort(
                _("unknown infinitepush store type specified %s") % storetype
            )

        indextype = repo.ui.config("infinitepush", "indextype", "")
        if indextype == "disk":
            from . import fileindex

            self.index = fileindex.fileindex(repo)
        elif indextype == "sql":
            # Delayed import of sqlindex to avoid including unnecessary
            # dependencies on mysql.connector.
            from . import sqlindex

            self.index = sqlindex.sqlindex(repo)
        else:
            raise error.Abort(
                _("unknown infinitepush index type specified %s") % indextype
            )


class filebundlestore(object):
    """bundle store in filesystem

    meant for storing bundles somewhere on disk and on network filesystems
    """

    def __init__(self, repo):
        self.storepath = repo.ui.configpath("scratchbranch", "storepath")
        if not self.storepath:
            self.storepath = repo.localvfs.join("scratchbranches", "filebundlestore")
        if not os.path.exists(self.storepath):
            os.makedirs(self.storepath)

    def _dirpath(self, hashvalue):
        """First two bytes of the hash are the name of the upper
        level directory, next two bytes are the name of the
        next level directory"""
        return os.path.join(self.storepath, hashvalue[0:2], hashvalue[2:4])

    def _filepath(self, filename):
        return os.path.join(self._dirpath(filename), filename)

    def write(self, data):
        filename = hashlib.sha1(data).hexdigest()
        dirpath = self._dirpath(filename)

        if not os.path.exists(dirpath):
            os.makedirs(dirpath)

        with open(self._filepath(filename), "w") as f:
            f.write(data)

        return filename

    def read(self, key):
        try:
            f = open(self._filepath(key), "r")
        except IOError:
            return None

        return f.read()


class externalbundlestore(object):
    def __init__(self, repo):
        """
        `put_binary` - path to binary file which uploads bundle to external
            storage and prints key to stdout
        `put_args` - format string with additional args to `put_binary`
                     {filename} replacement field can be used.
        `get_binary` - path to binary file which accepts filename and key
            (in that order), downloads bundle from store and saves it to file
        `get_args` - format string with additional args to `get_binary`.
                     {filename} and {handle} replacement field can be used.
        """
        ui = repo.ui

        # path to the binary which uploads a bundle to the external store
        # and prints the key to stdout.
        self.put_binary = ui.config("infinitepush", "put_binary")
        if not self.put_binary:
            raise error.Abort("put binary is not specified")
        # Additional args to ``put_binary``.  The '{filename}' replacement field
        # can be used to get the filename.
        self.put_args = ui.configlist("infinitepush", "put_args", [])

        # path to the binary which accepts a file and key (in that order) and
        # downloads the bundle form the store and saves it to the file.
        self.get_binary = ui.config("infinitepush", "get_binary")
        if not self.get_binary:
            raise error.Abort("get binary is not specified")
        # Additional args to ``get_binary``.  The '{filename}' and '{handle}'
        # replacement fields can be used to get the filename and key.
        self.get_args = ui.configlist("infinitepush", "get_args", [])

    def _call_binary(self, args):
        p = subprocess.Popen(
            args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, close_fds=True
        )
        stdout, stderr = p.communicate()
        returncode = p.returncode
        return returncode, stdout, stderr

    def write(self, data):
        # Won't work on windows because you can't open file second time without
        # closing it
        with NamedTemporaryFile() as temp:
            temp.write(data)
            temp.flush()
            temp.seek(0)
            formatted_args = [arg.format(filename=temp.name) for arg in self.put_args]
            returncode, stdout, stderr = self._call_binary(
                [self.put_binary] + formatted_args
            )

            if returncode != 0:
                raise error.Abort(
                    "Infinitepush failed to upload bundle to external store: %s"
                    % stderr
                )
            stdout_lines = stdout.splitlines()
            if len(stdout_lines) == 1:
                return stdout_lines[0]
            else:
                raise error.Abort(
                    "Infinitepush received bad output from %s: %s"
                    % (self.put_binary, stdout)
                )

    def read(self, handle):
        # Won't work on windows because you can't open file second time without
        # closing it
        with NamedTemporaryFile() as temp:
            formatted_args = [
                arg.format(filename=temp.name, handle=handle) for arg in self.get_args
            ]
            returncode, stdout, stderr = self._call_binary(
                [self.get_binary] + formatted_args
            )

            if returncode != 0:
                raise error.Abort("Failed to download from external store: %s" % stderr)
            return temp.read()
