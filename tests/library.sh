TESTDIR=${TESTDIR:-.}
if [[ ! -f "$TESTDIR/getdb.sh" ]]; then
  echo "skipped: getdb.sh missing. copy from getdb.sh.example and edit it"
  exit 80
fi

source "$TESTDIR/getdb.sh" >/dev/null

# Convert legacy fields from legacy getdb.sh implementation
if [[ -z $DBHOST && -z $DBPORT && -n $DBHOSTPORT ]]; then
    # Assuming they are set using the legacy way: $DBHOSTPORT
    DBHOST=`echo $DBHOSTPORT | cut -d : -f 1`
    DBPORT=`echo $DBHOSTPORT | cut -d : -f 2`
fi
[[ -z $DBHOST ]] && DBHOST=localhost
[[ -z $DBPORT ]] && DBPORT=3306
[[ -z $DBPASS && -n $PASSWORD ]] && DBPASS="$PASSWORD"
[[ -z $DBUSER && -n $USER ]] && DBUSER="$USER"
[[ -z $DBNAME ]] && DBNAME="testdb_hgsql_$$_$(date +%s)" && DBAUTODROP=1
[[ -n $DBPASS ]] && DBPASSOPT="-p$DBPASS"

MYSQLLOG="${MYSQLLOG:-/dev/null}"

mysql -h "$DBHOST" -P "$DBPORT" -u "$DBUSER" "$DBPASSOPT" &>> "$MYSQLLOG" <<EOF
CREATE DATABASE IF NOT EXISTS $DBNAME;
USE $DBNAME;
DROP TABLE IF EXISTS revisions;
DROP TABLE IF EXISTS revision_references;
$(cat $TESTDIR/../schema.sql)
EOF

function droptestdb() {
mysql -h "$DBHOST" -P "$DBPORT" -u "$DBUSER" "$DBPASSOPT" &>> "$MYSQLLOG" <<EOF
DROP DATABASE $DBNAME;
EOF
}

[[ $DBAUTODROP == 1 ]] && trap droptestdb EXIT

function initserver() {
  hg init --config extensions.hgsql=$TESTDIR/../hgsql.py $1
  configureserver $1 $2
}

configureserver() {
  cat >> $1/.hg/hgrc <<EOF
[extensions]
hgsql=$TESTDIR/../hgsql.py
strip=

[hgsql]
enabled = True
host = $DBHOST
database = $DBNAME
user = $DBUSER
password = $DBPASS
port = $DBPORT
reponame = $2

[server]
preferuncompressed=True
uncompressed=True

[ui]
ssh=python "$TESTDIR/dummyssh"
EOF
}

function initclient() {
  hg init $1
  configureclient $1
}

configureclient() {
  cat >> $1/.hg/hgrc <<EOF
[ui]
ssh=python "$TESTDIR/dummyssh"

[extensions]
hgsql=$TESTDIR/../hgsql.py
strip=
EOF
}
