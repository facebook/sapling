DBHOSTPORT=$($TESTDIR/getdb.sh)
DBHOST=`echo $DBHOSTPORT | cut -d : -f 1`
DBPORT=`echo $DBHOSTPORT | cut -d : -f 2`
DBNAME=`echo $DBHOSTPORT | cut -d : -f 3`

mysql -h $DBHOST -P $DBPORT -e "CREATE DATABASE $DBNAME;"
mysql -h $DBHOST -P $DBPORT -D $DBNAME -e 'CREATE TABLE revisions(
repo CHAR(32) BINARY NOT NULL,
path VARCHAR(256) BINARY NOT NULL,
chunk INT UNSIGNED NOT NULL,
chunkcount INT UNSIGNED NOT NULL,
linkrev INT UNSIGNED NOT NULL,
rev INT UNSIGNED NOT NULL,
node CHAR(40) BINARY NOT NULL,
entry BINARY(64) NOT NULL,
data0 CHAR(1) NOT NULL,
data1 LONGBLOB NOT NULL,
createdtime DATETIME NOT NULL,
INDEX linkrevs (repo, linkrev),
PRIMARY KEY (repo, path, rev, chunk)
);

CREATE TABLE revision_references(
autoid INT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
repo CHAR(32) BINARY NOT NULL,
namespace CHAR(32) BINARY NOT NULL,
name VARCHAR(256) BINARY,
value char(40) BINARY NOT NULL,
UNIQUE KEY bookmarkindex (repo, namespace, name)
);'


function initserver() {
  hg init $1
  cat >> $1/.hg/hgrc <<EOF
[extensions]
hgsql=$TESTDIR/../hgsql.py

[hgsql]
enabled = True
host = $DBHOST
database = $DBNAME
user = $USER
port = $DBPORT
reponame = $2

[ui]
ssh=python "$TESTDIR/dummyssh"
EOF
}

function initclient() {
  hg init $1
  cat >> $1/.hg/hgrc <<EOF
[ui]
ssh=python "$TESTDIR/dummyssh"
EOF
}
