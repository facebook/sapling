mkcommit() {
   echo "$1" > "$1"
   hg add "$1"
   hg ci -m "$1"
}

getid() {
   hg log --hidden --template '{node}\n' --rev "$1"
}

cat >> $HGRCPATH <<EOF
[alias]
debugobsolete=debugobsolete -d '0 0'
EOF
