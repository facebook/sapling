#!/bin/bash

scratchnodes() {
  for node in `find ../repo/.hg/scratchbranches/index/nodemap/* | sort`; do
     echo ${node##*/} `cat $node`
  done
}

scratchbookmarks() {
  for bookmark in `find ../repo/.hg/scratchbranches/index/bookmarkmap/* -type f | sort`; do
     echo "${bookmark##*/bookmarkmap/} `cat $bookmark`"
  done
}

setupcommon() {
  cat >> $HGRCPATH << EOF
[extensions]
commitcloud=
infinitepush=
pullcreatemarkers=
[ui]
ssh = python "$TESTDIR/dummyssh"
[infinitepush]
branchpattern=re:scratch/.*
bgssh = python "$TESTDIR/dummyssh" -bgssh
EOF
}

setupserver() {
cat >> .hg/hgrc << EOF
[infinitepush]
server=yes
indextype=disk
storetype=disk
reponame=babar
EOF
}

setupsqlclienthgrc() {
cat << EOF > .hg/hgrc
[ui]
ssh=python "$TESTDIR/dummyssh"
[extensions]
infinitepush=
[infinitepush]
branchpattern=re:scratch/.+
server=False
[paths]
default = ssh://user@dummy/server
EOF
}

setupsqlserverhgrc() {
cat << EOF > .hg/hgrc
[ui]
ssh=python "$TESTDIR/dummyssh"
[extensions]
infinitepush=
[infinitepush]
branchpattern=re:scratch/.+
server=True
indextype=sql
storetype=disk
reponame=$1
EOF
}

createdb() {
mysql -h $DBHOST -P $DBPORT -u $DBUSER $DBPASSOPT -e "CREATE DATABASE IF NOT EXISTS $DBNAME;" 2>/dev/null
mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT <<EOF
DROP TABLE IF EXISTS nodestobundle;
DROP TABLE IF EXISTS bookmarkstonode;
DROP TABLE IF EXISTS bundles;
DROP TABLE IF EXISTS nodesmetadata;
$(cat $TESTDIR/infinitepush/schema.sql)
EOF
}

setupdb() {
if ! ${PYTHON:-python} -c "import mysql.connector" 2>/dev/null; then
  echo "skipped: mysql-connector-python missing"
  exit 80
fi

GETDB_PATH="$TESTDIR/${HGTEST_GETDB_PATH:-getdb.sh}"

if [[ ! -f "$GETDB_PATH" ]]; then
  echo "skipped: getdb.sh missing"
  exit 80
fi

# shellcheck source=/dev/null
source "$GETDB_PATH" >/dev/null

if [[ -z $DBHOST && -z $DBPORT && -n $DBHOSTPORT ]]; then
    # Assuming they are set using the legacy way: $DBHOSTPORT
    DBHOST=`echo $DBHOSTPORT | cut -d : -f 1`
    DBPORT=`echo $DBHOSTPORT | cut -d : -f 2`
fi

[[ -z $DBHOST ]] && DBHOST=localhost
[[ -z $DBPORT ]] && DBPORT=3306
[[ -z $DBPASS && -n $PASSWORD ]] && DBPASS="$PASSWORD"
[[ -z $DBUSER && -n $USER ]] && DBUSER="$USER"
[[ -z $DBNAME ]] && DBNAME="testdb_hg_$$_$TIME"
if [[ -z $DBPASS ]]; then
    DBPASSOPT=''
else
    DBPASSOPT='-p'"$DBPASS"
fi

echo "sqlhost=$DBHOST:$DBPORT:$DBNAME:$DBUSER:$DBPASS" >> .hg/hgrc

createdb
}

waitbgbackup() {
  sleep 1
  hg debugwaitbackup
}

mkcommitautobackup() {
    echo $1 > $1
    hg add $1
    hg ci -m $1 --config infinitepushbackup.autobackup=True
}

setuplogdir() {
  mkdir $TESTTMP/logs
  chmod 0755 $TESTTMP/logs
  chmod +t $TESTTMP/logs
}

debugsshcall() {
  sed -n '/^running .*dummyssh.*$/p'
}
