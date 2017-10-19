#!/bin/sh
# setup config and various utility to test obsolescence marker exchanges tests

cat >> $TESTTMP/prune.sh << EOF
rev=\`hg log --hidden --template '{node}\n' --rev "\$3"\`

hg debugobsolete --record-parents \$1 "\$2" \$rev \
   && hg up --quiet 'max((::.) - obsolete())'
EOF

cat >> $HGRCPATH <<EOF
[web]
# We test http pull and push, drop authentication requirement
push_ssl = false
allow_push = *

[ui]
# simpler log output
logtemplate ="{node|short} ({phase}): {desc}\n"

[phases]
# non publishing server
publish=False

[experimental]
# reduce output changes
bundle2-output-capture=True
# enable evolution
evolution=true

[extensions]
# we need to strip some changeset for some test cases
hgext.strip=

[devel]
strip-obsmarkers = no

[alias]
# fix date used to create obsolete markers.
debugobsolete=debugobsolete -d '0 0'
# poor man substiture to the evolve 'hg prune'. using prune makes the test clearer and 
prune = !sh $TESTTMP/prune.sh \$1 "\$2" "\$3"
EOF

mkcommit() {
   echo "$1" > "$1"
   hg add "$1"
   hg ci -m "$1"
}
getid() {
   hg log --hidden --template '{node}\n' --rev "$1"
}

setuprepos() {
    echo creating test repo for test case $1
    mkdir $1
    cd $1
    echo - pulldest
    hg init pushdest
    cd pushdest
    mkcommit O
    hg phase --public .
    cd ..
    echo - main
    hg clone -q pushdest main
    echo - pushdest
    hg clone -q main pulldest
    echo 'cd into `main` and proceed with env setup'
}

inspect_obsmarkers (){
    # This exist as its own function to help the evolve extension reuse the tests as is.
    # The evolve extensions version will includes more advances query (eg:
    # related to obsmarkers discovery) to this.
    echo 'obsstore content'
    echo '================'
    hg debugobsolete
}

dotest() {
    # dotest TESTNAME [TARGETNODE] [PUSHFLAGS+]
    #
    # test exchange for the given test case.
    #
    # This function performs push and pull in all directions through all
    # protocols and display the resulting obsolescence markers on all sides.

    testcase=$1
    shift
    target="$1"
    if [ $# -gt 0 ]; then
        shift
    fi
    targetnode=""
    desccall=""
    cd $testcase
    echo "## Running testcase $testcase"
    if [ -n "$target" ]; then
        desccall="desc("\'"$target"\'")"
        targetnode="`hg -R main id -qr \"$desccall\"`"
        echo "# testing echange of \"$target\" ($targetnode)"
    fi
    echo "## initial state"
    echo "# obstore: main"
    hg -R main     debugobsolete | sort
    echo "# obstore: pushdest"
    hg -R pushdest debugobsolete | sort
    echo "# obstore: pulldest"
    hg -R pulldest debugobsolete | sort

    if [ -n "$target" ]; then
        echo "## pushing \"$target\"" from main to pushdest
        hg -R main push -r "$desccall" $@ pushdest
    else
        echo "## pushing from main to pushdest"
        hg -R main push pushdest $@
    fi
    echo "## post push state"
    echo "# obstore: main"
    hg -R main     debugobsolete | sort
    echo "# obstore: pushdest"
    hg -R pushdest debugobsolete | sort
    echo "# obstore: pulldest"
    hg -R pulldest debugobsolete | sort
    if [ -n "$target" ]; then
        echo "## pulling \"$targetnode\"" from main into pulldest
        hg -R pulldest pull -r $targetnode $@ main
    else
        echo "## pulling from main into pulldest"
        hg -R pulldest pull main $@
    fi
    echo "## post pull state"
    echo "# obstore: main"
    hg -R main     debugobsolete | sort
    echo "# obstore: pushdest"
    hg -R pushdest debugobsolete | sort
    echo "# obstore: pulldest"
    hg -R pulldest debugobsolete | sort

    cd ..

}
