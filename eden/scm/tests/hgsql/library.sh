# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

cat >> $HGRCPATH << EOF
[experimental]
narrow-heads = false
EOF

HGSQL_TESTDIR="${RUN_TESTS_LIBRARY:-"${TESTDIR:-.}"}"

GETDB_PATH="${HGTEST_GETDB_SH:-$HGSQL_TESTDIR}/getdb.sh"
export DUMMYSSH_STABLE_ORDER=1

if [[ ! -f "$GETDB_PATH" ]]; then
  echo "skipped: getdb.sh missing. copy from getdb.sh.example and edit it"
  exit 80
fi

if ! hg debugpython -- -c "import mysql.connector" 2>/dev/null; then
  echo "skipped: mysql-connector-python missing"
  exit 80
fi

source "$GETDB_PATH" >/dev/null

[[ -z $DBHOST ]] && DBHOST=localhost
[[ -z $DBPORT ]] && DBPORT=3306
[[ -z $DBENGINE ]] && DBENGINE=innodb
[[ -z $DBPASS && -n $PASSWORD ]] && DBPASS="$PASSWORD"
[[ -z $DBUSER && -n $USER ]] && DBUSER="$USER"
[[ -z $DBNAME ]] && DBNAME="testdb_hgsql_$$_$(date +%s)" && DBAUTODROP=1
[[ -n $DBPASS ]] && DBPASSOPT="-p$DBPASS"

MYSQLLOG="${MYSQLLOG:-/dev/null}"

# skip if DBENGINE is not supported
( mysql -h "$DBHOST" -P "$DBPORT" -u "$DBUSER" "$DBPASSOPT" \
  --execute='SHOW ENGINES' --silent --skip-column-names  | \
  egrep -iq "^$DBENGINE[[:space:]](default|yes)[[:space:]]" )

if [[ $? != 0 ]]; then
  echo "skipped: $DBENGINE unsupported"
  exit 80
fi

mysql -h "$DBHOST" -P "$DBPORT" -u "$DBUSER" "$DBPASSOPT" &>> "$MYSQLLOG" <<EOF
CREATE DATABASE IF NOT EXISTS $DBNAME;
USE $DBNAME;
DROP TABLE IF EXISTS revisions;
DROP TABLE IF EXISTS revision_references;
$(cat $HGSQL_TESTDIR/hgsql/schema.$DBENGINE.sql)
EOF

if [[ $? != 0 ]]; then
  echo "skipped: unable to initialize the database. check your getdb.sh"
  exit 80
fi

function droptestdb() {
mysql -h "$DBHOST" -P "$DBPORT" -u "$DBUSER" "$DBPASSOPT" &>> "$MYSQLLOG" <<EOF
DROP DATABASE $DBNAME;
EOF
}

[[ $DBAUTODROP == 1 ]] && trap droptestdb EXIT

function initserver() {
  hg init --config extensions.hgsql= $1
  configureserver $1 $2
}

configureserver() {
  cat >> $1/.hg/hgrc <<EOF
[extensions]
hgsql=

[hgsql]
enabled = True
host = $DBHOST
database = $DBNAME
user = $DBUSER
password = $DBPASS
port = $DBPORT
reponame = $2
engine = $DBENGINE

[server]
preferuncompressed=True
uncompressed=True

[ui]
ssh=$(dummysshcmd)
EOF
}

function initclient() {
  hg init --config extensions.hgsql=! --config format.usehgsql=false $1
  configureclient $1
}

configureclient() {
  cat >> $1/.hg/hgrc <<EOF
[ui]
ssh=$(dummysshcmd)
EOF
}
