DBHOSTPORT=$($TESTDIR/getdb.sh)
DBHOST=`echo $DBHOSTPORT | cut -d : -f 1`
DBPORT=`echo $DBHOSTPORT | cut -d : -f 2`
DBNAME=`echo $DBHOSTPORT | cut -d : -f 3`
DBUSER=`echo $DBHOSTPORT | cut -d : -f 4`
DBPASS=`echo $DBHOSTPORT | cut -d : -f 5-`

mysql -h $DBHOST -P $DBPORT -u $DBUSER -p"$DBPASS" -e "
CREATE DATABASE IF NOT EXISTS $DBNAME;" 2>/dev/null
mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER -p"$DBPASS" -e '
DROP TABLE IF EXISTS Moves;' 2>/dev/null

function initserver() {
  cat >> $1/.hg/hgrc <<EOF
[copytrace]
xdbhost = $DBHOST
xdb = $DBNAME
xdbuser = $DBUSER
xdbpassword = $DBPASS
xdbport = $DBPORT
EOF
}
