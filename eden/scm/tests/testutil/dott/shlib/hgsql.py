# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# Ported from tests/hgsql/library.sh

from __future__ import absolute_import

import os
import subprocess

from .. import shlib, testtmp


try:
    from ... import getdb
except ImportError:
    import sys

    sys.exit(80)

dbconfig = None


def _createdatabase():
    schema = open(
        shlib.expandpath("$TESTDIR/hgsql/schema.%s.sql" % dbconfig["dbengine"]), "rb"
    ).read()

    p = subprocess.Popen(
        [
            "mysql",
            "-h%s" % dbconfig["dbhost"],
            "-P%s" % dbconfig["dbport"],
            "-u%s" % dbconfig["dbuser"],
            "-p%s" % dbconfig["dbpass"],
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    stdout, stderr = p.communicate(
        r"""
    CREATE DATABASE IF NOT EXISTS {dbname};
    USE {dbname};
    DROP TABLE IF EXISTS revisions;
    DROP TABLE IF EXISTS revision_references;
    DROP TABLE IF EXISTS repo_lock;
    {schema}
    """.format(
            dbname=dbconfig["dbname"], dbengine=dbconfig["dbengine"], schema=schema
        )
    )
    if p.returncode != 0:
        raise RuntimeError("failed to create mysql database: %s\n%s" % (stdout, stderr))


def initdb():
    global dbconfig
    dbconfig = getdb.get_db_config()
    _createdatabase()


def initserver(servername, dbname):
    shlib.hg("init", "--config=extensions.hgsql=", servername)
    configureserver(servername, dbname)


def configureserver(servername, reponame):
    config = dict(dbconfig)
    config["reponame"] = reponame
    open(os.path.join(servername, ".hg/hgrc"), "ab").write(
        r"""
[extensions]
hgsql=

[hgsql]
enabled = True
host = {dbhost}
database = {dbname}
user = {dbuser}
password = {dbpass}
port = {dbport}
reponame = {reponame}
engine = {dbengine}

[server]
preferuncompressed=True
uncompressed=True

[ui]
ssh=python "$TESTDIR/dummyssh"
""".format(
            **config
        )
    )


def initclient(name):
    shlib.hg("init", name)
    configureclient(name)


def configureclient(name):
    open(os.path.join(name, ".hg/hgrc"), "ab").write(
        r"""
[ui]
ssh=python "$TESTDIR/dummyssh"
"""
    )
