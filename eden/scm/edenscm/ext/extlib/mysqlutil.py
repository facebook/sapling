# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# mysqlutil.py - useful utility methods for accessing mysql from server-side hg


from typing import Any, Dict


class InvalidConnectionString(Exception):
    pass


def parseconnectionstring(connstr) -> Dict[str, Any]:
    """
    Parses connection string in format 'IP:PORT:DB_NAME:USER:PASSWORD' and return
    parameters for mysql.connection module
    """

    try:
        host, port, db, user, password = connstr.rsplit(":", 4)
        return {
            "host": host,
            "port": port,
            "database": db,
            "user": user,
            "password": password,
        }
    except ValueError:
        raise InvalidConnectionString()


def insert(sqlconn, tablename, argsdict) -> None:
    """
    Inserts new row into a table, given a name of a table and a mapping
    column name -> column value
    """

    sqlcursor = sqlconn.cursor()

    items = list(argsdict.items())
    columns = ", ".join(
        ("{column_name}".format(column_name=column_name) for column_name, _ in items)
    )

    placeholders = ", ".join(("%s" for _ in items))

    insertstmt = "INSERT INTO {table} ({columns}) VALUES ({placeholders})".format(
        table=tablename, columns=columns, placeholders=placeholders
    )

    sqlcursor.execute(insertstmt, params=[value for _, value in items])
    sqlconn.commit()
