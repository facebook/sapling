# bash completion for the Mercurial distributed SCM -*- sh -*-

# Docs:
#
# If you source this file from your .bashrc, bash should be able to
# complete a command line that uses hg with all the available commands
# and options and sometimes even arguments.
#
# Mercurial allows you to define additional commands through extensions.
# Bash should be able to automatically figure out the name of these new
# commands and their options.  See below for how to define _hg_opt_foo
# and _hg_cmd_foo functions to fine-tune the completion for option and
# non-option arguments, respectively.
#
#
# Notes about completion for specific commands:
#
# - the completion function for the email command from the patchbomb
#   extension will try to call _hg_emails to get a list of e-mail
#   addresses.  It's up to the user to define this function.  For
#   example, put the addresses of the lists that you usually patchbomb
#   in ~/.patchbomb-to and the addresses that you usually use to send
#   the patchbombs in ~/.patchbomb-from and use something like this:
#
#      _hg_emails()
#      {
#          if [ -r ~/.patchbomb-$1 ]; then
#              cat ~/.patchbomb-$1
#          fi
#      }
#
#
# Writing completion functions for additional commands:
#
# If it exists, the function _hg_cmd_foo will be called without
# arguments to generate the completion candidates for the hg command
# "foo".  If the command receives some arguments that aren't options
# even though they start with a "-", you can define a function called
# _hg_opt_foo to generate the completion candidates.  If _hg_opt_foo
# doesn't return 0, regular completion for options is attempted.
#
# In addition to the regular completion variables provided by bash,
# the following variables are also set:
# - $hg - the hg program being used (e.g. /usr/bin/hg)
# - $cmd - the name of the hg command being completed
# - $cmd_index - the index of $cmd in $COMP_WORDS
# - $cur - the current argument being completed
# - $prev - the argument before $cur
# - $global_args - "|"-separated list of global options that accept
#                  an argument (e.g. '--cwd|-R|--repository')
# - $canonical - 1 if we canonicalized $cmd before calling the function
#                0 otherwise
#

shopt -s extglob

_hg_cmd()
{
    HGPLAIN=1 "$hg" "$@" 2>/dev/null
}

_hg_commands()
{
    local commands
    commands="$(HGPLAINEXCEPT=alias _hg_cmd debugcomplete "$cur")" || commands=""
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$commands' -- "$cur"))
}

_hg_paths()
{
    local paths="$(_hg_cmd paths -q)"
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$paths' -- "$cur"))
}

_hg_repos()
{
    local i
    for i in $(compgen -d -- "$cur"); do
        test ! -d "$i"/.hg || COMPREPLY=(${COMPREPLY[@]:-} "$i")
    done
}

_hg_debugpathcomplete()
{
    local files="$(_hg_cmd debugpathcomplete $1 "$cur")"
    local IFS=$'\n'
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$files' -- "$cur"))
}

_hg_status()
{
    local files="$(_hg_cmd status -n$1 "glob:$cur**")"
    local IFS=$'\n'
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$files' -- "$cur"))
}

_hg_branches()
{
    local branches="$(_hg_cmd branches -q)"
    local IFS=$'\n'
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$branches' -- "$cur"))
}

_hg_bookmarks()
{
    local bookmarks="$(_hg_cmd bookmarks -q)"
    local IFS=$'\n'
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$bookmarks' -- "$cur"))
}

_hg_labels()
{
    local labels="$(_hg_cmd debugnamecomplete "$cur")"
    local IFS=$'\n'
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$labels' -- "$cur"))
}

# this is "kind of" ugly...
_hg_count_non_option()
{
    local i count=0
    local filters="$1"

    for ((i=1; $i<=$COMP_CWORD; i++)); do
        if [[ "${COMP_WORDS[i]}" != -* ]]; then
            if [[ ${COMP_WORDS[i-1]} == @($filters|$global_args) ]]; then
                continue
            fi
            count=$(($count + 1))
        fi
    done

    echo $(($count - 1))
}

_hg_fix_wordlist()
{
    local LASTCHAR=' '
    if [ ${#COMPREPLY[@]} = 1 ]; then
        [ -d "$COMPREPLY" ] && LASTCHAR=/
        COMPREPLY=$(printf %q%s "$COMPREPLY" "$LASTCHAR")
    else
        for ((i=0; i < ${#COMPREPLY[@]}; i++)); do
            [ -d "${COMPREPLY[$i]}" ] && COMPREPLY[$i]=${COMPREPLY[$i]}/
        done
    fi
}

_hg()
{
    local cur prev cmd cmd_index opts i aliashg
    # global options that receive an argument
    local global_args='--cwd|-R|--repository'
    local hg="$1"
    local canonical=0

    aliashg=$(alias $hg 2>/dev/null)
    if [[ -n "$aliashg" ]]; then
      aliashg=${aliashg#"alias $hg='"}
      aliashg=${aliashg%"'"}
      hg=$aliashg
    fi

    COMPREPLY=()
    cur="$2"
    prev="$3"

    # searching for the command
    # (first non-option argument that doesn't follow a global option that
    #  receives an argument)
    for ((i=1; $i<=$COMP_CWORD; i++)); do
        if [[ ${COMP_WORDS[i]} != -* ]]; then
            if [[ ${COMP_WORDS[i-1]} != @($global_args) ]]; then
                cmd="${COMP_WORDS[i]}"
                cmd_index=$i
                break
            fi
        fi
    done

    if [[ "$cur" == -* ]]; then
        if [ "$(type -t "_hg_opt_$cmd")" = function ] && "_hg_opt_$cmd"; then
            _hg_fix_wordlist
            return
        fi

        opts=$(_hg_cmd debugcomplete --options "$cmd")

        COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$opts' -- "$cur"))
        _hg_fix_wordlist
        return
    fi

    # global options
    case "$prev" in
        -R|--repository)
            _hg_paths
            _hg_repos
            _hg_fix_wordlist
            return
        ;;
        --cwd)
            # Stick with default bash completion
            _hg_fix_wordlist
            return
        ;;
    esac

    if [ -z "$cmd" ] || [ $COMP_CWORD -eq $i ]; then
        _hg_commands
        _hg_fix_wordlist
        return
    fi

    # try to generate completion candidates for whatever command the user typed
    local help
    if _hg_command_specific; then
        _hg_fix_wordlist
        return
    fi

    # canonicalize the command name and try again
    help=$(_hg_cmd help "$cmd")
    if [ $? -ne 0 ]; then
        # Probably either the command doesn't exist or it's ambiguous
        return
    fi
    cmd=${help#hg }
    cmd=${cmd%%[$' \n']*}
    canonical=1
    _hg_command_specific
    _hg_fix_wordlist
}

_hg_command_specific()
{
    if [ "$(type -t "_hg_cmd_$cmd")" = function ]; then
        "_hg_cmd_$cmd"
        return 0
    fi

    if [ "$cmd" != status ]; then
        case "$prev" in
            -r|--rev)
                if [[ $canonical = 1 || status != "$cmd"* ]]; then
                    _hg_labels
                    return 0
                fi
                return 1
            ;;
            -B|--bookmark)
                if [[ $canonical = 1 || status != "$cmd"* ]]; then
                    _hg_bookmarks
                    return 0
                fi
                return 1
            ;;
            -b|--branch)
                if [[ $canonical = 1 || status != "$cmd"* ]]; then
                    _hg_branches
                    return 0
                fi
                return 1
            ;;
        esac
    fi

    local aliascmd=$(_hg_cmd showconfig alias.$cmd | awk '{print $1}')
    [ -n "$aliascmd" ] && cmd=$aliascmd

    case "$cmd" in
        help)
            _hg_commands
        ;;
        export)
            if _hg_ext_mq_patchlist qapplied && [ "${COMPREPLY[*]}" ]; then
                return 0
            fi
            _hg_labels
        ;;
        manifest|update|up|checkout|co)
            _hg_labels
        ;;
        pull|push|outgoing|incoming)
            _hg_paths
            _hg_repos
        ;;
        paths)
            _hg_paths
        ;;
        add)
            _hg_status "u"
        ;;
        merge)
            _hg_labels
        ;;
        commit|ci|record)
            _hg_status "mar"
        ;;
        remove|rm)
            _hg_debugpathcomplete -n
        ;;
        forget)
            _hg_debugpathcomplete -fa
        ;;
        diff)
            _hg_status "mar"
        ;;
        revert)
            _hg_debugpathcomplete
        ;;
        clone)
            local count=$(_hg_count_non_option)
            if [ $count = 1 ]; then
                _hg_paths
            fi
            _hg_repos
        ;;
        debugindex|debugindexdot)
            COMPREPLY=(${COMPREPLY[@]:-} $(compgen -f -X "!*.i" -- "$cur"))
        ;;
        debugdata)
            COMPREPLY=(${COMPREPLY[@]:-} $(compgen -f -X "!*.d" -- "$cur"))
        ;;
        *)
            return 1
        ;;
    esac

    return 0
}

complete -o bashdefault -o default -o nospace -F _hg hg \
    || complete -o default -o nospace -F _hg hg


# Completion for commands provided by extensions

# bookmarks
_hg_cmd_bookmarks()
{
    _hg_bookmarks
    return
}

# mq
_hg_ext_mq_patchlist()
{
    local patches
    patches=$(_hg_cmd $1)
    if [ $? -eq 0 ] && [ "$patches" ]; then
        COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$patches' -- "$cur"))
        return 0
    fi
    return 1
}

_hg_ext_mq_queues()
{
    local root=$(_hg_cmd root)
    local n
    for n in $(cd "$root"/.hg && compgen -d -- "$cur"); do
        # I think we're usually not interested in the regular "patches" queue
        # so just filter it.
        if [ "$n" != patches ] && [ -e "$root/.hg/$n/series" ]; then
            COMPREPLY=(${COMPREPLY[@]:-} "$n")
        fi
    done
}

_hg_cmd_qpop()
{
    if [[ "$prev" = @(-n|--name) ]]; then
        _hg_ext_mq_queues
        return
    fi
    _hg_ext_mq_patchlist qapplied
}

_hg_cmd_qpush()
{
    if [[ "$prev" = @(-n|--name) ]]; then
        _hg_ext_mq_queues
        return
    fi
    _hg_ext_mq_patchlist qunapplied
}

_hg_cmd_qgoto()
{
    if [[ "$prev" = @(-n|--name) ]]; then
        _hg_ext_mq_queues
        return
    fi
    _hg_ext_mq_patchlist qseries
}

_hg_cmd_qdelete()
{
    local qcmd=qunapplied
    if [[ "$prev" = @(-r|--rev) ]]; then
        qcmd=qapplied
    fi
    _hg_ext_mq_patchlist $qcmd
}

_hg_cmd_qfinish()
{
    if [[ "$prev" = @(-a|--applied) ]]; then
        return
    fi
    _hg_ext_mq_patchlist qapplied
}

_hg_cmd_qsave()
{
    if [[ "$prev" = @(-n|--name) ]]; then
        _hg_ext_mq_queues
        return
    fi
}

_hg_cmd_rebase() {
   if [[ "$prev" = @(-s|--source|-d|--dest|-b|--base|-r|--rev) ]]; then
       _hg_labels
       return
   fi
}

_hg_cmd_strip()
{
    if [[ "$prev" = @(-B|--bookmark) ]]; then
        _hg_bookmarks
        return
    fi
    _hg_labels
}

_hg_cmd_qcommit()
{
    local root=$(_hg_cmd root)
    # this is run in a sub-shell, so we can't use _hg_status
    local files=$(cd "$root/.hg/patches" && _hg_cmd status -nmar)
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$files' -- "$cur"))
}

_hg_cmd_qfold()
{
    _hg_ext_mq_patchlist qunapplied
}

_hg_cmd_qrename()
{
    _hg_ext_mq_patchlist qseries
}

_hg_cmd_qheader()
{
    _hg_ext_mq_patchlist qseries
}

_hg_cmd_qclone()
{
    local count=$(_hg_count_non_option)
    if [ $count = 1 ]; then
        _hg_paths
    fi
    _hg_repos
}

_hg_ext_mq_guards()
{
    _hg_cmd qselect --series | sed -e 's/^.//'
}

_hg_cmd_qselect()
{
    local guards=$(_hg_ext_mq_guards)
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$guards' -- "$cur"))
}

_hg_cmd_qguard()
{
    local prefix=''

    if [[ "$cur" == +* ]]; then
        prefix=+
    elif [[ "$cur" == -* ]]; then
        prefix=-
    fi
    local ncur=${cur#[-+]}

    if ! [ "$prefix" ]; then
        _hg_ext_mq_patchlist qseries
        return
    fi

    local guards=$(_hg_ext_mq_guards)
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -P $prefix -W '$guards' -- "$ncur"))
}

_hg_opt_qguard()
{
    local i
    for ((i=cmd_index+1; i<=COMP_CWORD; i++)); do
        if [[ ${COMP_WORDS[i]} != -* ]]; then
            if [[ ${COMP_WORDS[i-1]} != @($global_args) ]]; then
                _hg_cmd_qguard
                return 0
            fi
        elif [ "${COMP_WORDS[i]}" = -- ]; then
            _hg_cmd_qguard
            return 0
        fi
    done
    return 1
}

_hg_cmd_qqueue()
{
    local q
    local queues
    local opts="--list --create --rename --delete --purge"

    queues=$( _hg_cmd qqueue --quiet )

    COMPREPLY=( $( compgen -W "${opts} ${queues}" "${cur}" ) )
}


# hbisect
_hg_cmd_bisect()
{
    local i subcmd

    # find the sub-command
    for ((i=cmd_index+1; i<=COMP_CWORD; i++)); do
        if [[ ${COMP_WORDS[i]} != -* ]]; then
            if [[ ${COMP_WORDS[i-1]} != @($global_args) ]]; then
                subcmd="${COMP_WORDS[i]}"
                break
            fi
        fi
    done

    if [ -z "$subcmd" ] || [ $COMP_CWORD -eq $i ] || [ "$subcmd" = help ]; then
        COMPREPLY=(${COMPREPLY[@]:-}
                   $(compgen -W 'bad good help init next reset' -- "$cur"))
        return
    fi

    case "$subcmd" in
        good|bad)
            _hg_labels
            ;;
    esac

    return
}


# patchbomb
_hg_cmd_email()
{
    case "$prev" in
        -c|--cc|-t|--to|-f|--from|--bcc)
            # we need an e-mail address. let the user provide a function
            # to get them
            if [ "$(type -t _hg_emails)" = function ]; then
                local arg=to
                if [[ "$prev" == @(-f|--from) ]]; then
                    arg=from
                fi
                local addresses=$(_hg_emails $arg)
                COMPREPLY=(${COMPREPLY[@]:-}
                           $(compgen -W '$addresses' -- "$cur"))
            fi
            return
            ;;
        -m|--mbox)
            # fallback to standard filename completion
            return
            ;;
        -s|--subject)
            # free form string
            return
            ;;
    esac

    _hg_labels
    return
}


# gpg
_hg_cmd_sign()
{
    _hg_labels
}


# transplant
_hg_cmd_transplant()
{
    case "$prev" in
        -s|--source)
            _hg_paths
            _hg_repos
            return
            ;;
        --filter)
            # standard filename completion
            return
            ;;
    esac

    # all other transplant options values and command parameters are revisions
    _hg_labels
    return
}

# shelve
_hg_shelves()
{
    local shelves="$(_hg_cmd shelve -ql)"
    local IFS=$'\n'
    COMPREPLY=(${COMPREPLY[@]:-} $(compgen -W '$shelves' -- "$cur"))
}

_hg_cmd_shelve()
{
    if [[ "$prev" = @(-d|--delete|-l|--list|-p|--patch|--stat) ]]; then
        _hg_shelves
    else
        _hg_status "mard"
    fi
}

_hg_cmd_unshelve()
{
    _hg_shelves
}
