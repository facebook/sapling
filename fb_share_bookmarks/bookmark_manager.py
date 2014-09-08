from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals

import argparse
import getpass
import json
import socket
import sys

from libfb import db_locator

# Use this link to setup a local mysql server on your devserver
# https://our.intern.facebook.com/intern/wiki/index.php/DBA/devservers
MYSQL_DATABASE = 'devdb.mjberger'

class DbManager:

    def __init__(self, user, server):
        self._user = user
        self._server = server

    def push(self, name, dirPath):
        raise Exception('push has not been implemented')

    def pull(self, name):
        raise Exception('pull has not been implemented')

    def generate_name(self, bookmark, project):
        return "{}/{}/{}".format(project, self._user, bookmark)

class MysqlDbManager(DbManager):

    def __init__(self, user, server):
        DbManager.__init__(self, user, server)
        locator = db_locator.Locator(tier_name=MYSQL_DATABASE, writable=True)
        locator.populate_connection_params()
        locator.populate_auth_credentials()
        self._conn = locator.create_connection()

    def __enter__(self):
        pass

    def __exit__(self, type, value, traceback):
        self._conn.close()

    def push(self, name, dirPath):
        try:
            args = [
                name,
                self._server,
                dirPath
            ]
            c = self._conn.cursor()
            q = ("INSERT INTO hgBookmarks(name, server, path) "
                 "VALUES(%s, %s, %s) ON DUPLICATE KEY UPDATE "
                 "server=VALUES(server), path=VALUES(path);")

            c.execute(q, args)
            c.close()
            self._conn.commit()
            return True

        except Exception as e:
            return False

    def pull(self, name):
        q = "SELECT * FROM hgBookmarks WHERE name=%s"
        c = self._conn.cursor()
        c.execute(q, [name])
        rows = c.fetchall()
        c.close()
        return rows

    def delete(self, name):
        q = "DELETE FROM hgBookmarks WHERE name=%s"
        c = self._conn.cursor()
        c.execute(q, [name])
        c.close()

    def get_version(self):
        self._conn.query('SELECT VERSION()')
        result = self._conn.use_result()
        return result.fetch_row()[0][0]

    def close(self):
        self._conn.close()

def parse_args(args):
    action = None
    if args[0] == 'push':
        action = 'push'
    elif args[0] == 'pull':
        action = 'pull'
    elif args[0] == 'delete':
        action = 'delete'

    args = args[1:]
    ap = argparse.ArgumentParser()
    ap.add_argument('-N', '--name',
                    help='Name of db entry, where '
                    'name is {project}/{user}/{bookmark}')
    ap.add_argument('-p', '--path',
                    help='absolute path to hg repo')
    args = ap.parse_args(args)
    args.action = action
    return args

def checkAttributes(args, l):
    for item in l:
        if vars(args)[item] is None:
            return False
    return True

def canPush(args):
    return checkAttributes(args, ['name', 'path'])

def canPull(args):
    return checkAttributes(args, ['name'])

def canDelete(args):
    return checkAttributes(args, ['name'])

def main():
    if len(sys.argv) > 1:
        args = parse_args(sys.argv[1:])
        if args.action is None:
            print('Please use push or pull.')
            return
    else:
        print('Please use push, pull, or delete.')
        sys.exit(0)

    if not args.name:
        print('All actions require a name')
        sys.exit(1)

    user = getpass.getuser()
    host = socket.gethostname()
    m = MysqlDbManager(user, host)
    with m:
        if args.action == 'push':
            if not canPush(args):
                print('Error: name and path are required for push')
                sys.exit(1)
            success = m.push(args.name, args.path)
            if success:
                print('Successfully pushed commit info to database')
            else:
                print('Unable to push commit info to database')
                sys.exit(1)

        elif args.action == 'pull':
            row = m.pull(args.name)
            if row:
                row = row[0]
                name = row[0].split('/')
                output = {
                    'project': name[0],
                    'user': name[1],
                    'bookmark': name[2],
                    'server': row[1],
                    'root': row[2]
                }
                print(json.dumps(output))
            else:
                print('Entry does not exist.')
                sys.exit(1)

        elif args.action == 'delete':
            m.delete(args.name)

if __name__ == '__main__':
    main()
