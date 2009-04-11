function verify_current_revision()
{
    /bin/rm -rf *
    exportcmd="svn export `hg svn info 2> /dev/null | grep '^URL: ' | sed 's/URL: //'` -r`hg svn info | grep ^Revision | sed 's/.*: //;s/ .*//'` . --force"
    `echo $exportcmd` > /dev/null
    x=$?
    if [[ "$x" != "0" ]] ; then
        echo $exportcmd
        echo 'export failed!'
        return 255
    fi
    if [[ "`hg st | wc -l | python -c 'import sys; print sys.stdin.read().strip()'`" == "0" ]] ; then
        return 0
    else
        if [[ $1 != "keep" ]] ; then
            revert_all_files
        fi
        return 1
    fi
}

function revert_all_files()
{
    hg revert --all
    hg purge
}
