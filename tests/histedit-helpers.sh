fixbundle() {
    grep -v 'saving bundle' | grep -v 'saved backup' | \
        grep -v added | grep -v adding | \
        grep -v "unable to find 'e' for patching" | \
        grep -v "e: No such file or directory"
}
