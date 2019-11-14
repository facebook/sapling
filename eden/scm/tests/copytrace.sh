function initclient() {
  cat >> $1/.hg/hgrc <<EOF
[copytrace]
remote = False
enablefilldb = True
fastcopytrace = True
[experimental]
copytrace = off
EOF
}
