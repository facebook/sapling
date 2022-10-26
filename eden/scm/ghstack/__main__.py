import argparse

import ghstack
import ghstack.action
import ghstack.checkout
import ghstack.config
import ghstack.github_real
import ghstack.land
import ghstack.logs
import ghstack.rage
import ghstack.submit
import ghstack.unlink


def main() -> None:
    parser = argparse.ArgumentParser(
        description='Submit stack of diffs to GitHub.')
    parser.add_argument(
        '--version', action='version',
        version="ghstack {}".format(ghstack.__version__),
        help='Print version')
    parser.add_argument(
        '--debug', action='store_true',
        help='Log debug information to stderr')

    subparsers = parser.add_subparsers(dest='cmd')

    submit = subparsers.add_parser('submit')
    for subparser in (submit, parser):
        subparser.add_argument(
            '--message', '-m',
            default='Update',
            help='Description of change you made')
        subparser.add_argument(
            '--update-fields', '-u', action='store_true',
            help='Update GitHub pull request summary from the local commit')
        subparser.add_argument(
            '--short', action='store_true',
            help='Print only the URL of the latest opened PR to stdout')
        subparser.add_argument(
            '--force', action='store_true',
            help='force push the branch even if your local branch is stale')
        subparser.add_argument(
            '--no-skip', action='store_true',
            help='Never skip pushing commits, even if the contents didn\'t change '
                 '(use this if you\'ve only updated the commit message).')
        subparser.add_argument(
            '--draft', action='store_true',
            help='Create the pull request in draft mode (only if it has not already been created)')

    unlink = subparsers.add_parser('unlink')
    unlink.add_argument('COMMITS', nargs='*')

    rage = subparsers.add_parser('rage')
    rage.add_argument('--latest', action='store_true',
        help='Select the last command (not including rage commands) to report')

    land = subparsers.add_parser('land')
    land.add_argument('pull_request', metavar='PR',
        help='GitHub pull request URL of stack to land')

    checkout = subparsers.add_parser('checkout')
    checkout.add_argument('pull_request', metavar='PR',
        help='GitHub pull request URL to checkout')

    action = subparsers.add_parser('action')
    # TODO: support number as well
    action.add_argument('pull_request', metavar='PR',
        help='GitHub pull request URL to perform action on')
    action.add_argument('--close', action='store_true',
        help='Close the specified pull request')

    args = parser.parse_args()

    if args.cmd is None:
        args.cmd = 'submit'

    with ghstack.logs.manager(debug=args.debug):

        conf = ghstack.config.read_config()
        sh = create_shell(conf)
        oauth_token = conf.github_oauth
        if not oauth_token:
            raise RuntimeError(
                "ghstack.config.read_config() should guarantee github_oauth is set"
            )
        github = ghstack.github_real.RealGitHubEndpoint(
            oauth_token=oauth_token,
            proxy=conf.proxy,
            github_url=conf.github_url,
        )

        if args.cmd == 'rage':
            ghstack.rage.main(latest=args.latest)
        elif args.cmd == 'submit':
            ghstack.submit.main(
                msg=args.message,
                username=conf.github_username,
                sh=sh,
                github=github,
                update_fields=args.update_fields,
                short=args.short,
                force=args.force,
                no_skip=args.no_skip,
                draft=args.draft,
                github_url=conf.github_url,
                remote_name=conf.remote_name,
            )
        elif args.cmd == 'unlink':
            ghstack.unlink.main(
                commits=args.COMMITS,
                github=github,
                sh=sh,
                github_url=conf.github_url,
                remote_name=conf.remote_name,
            )
        elif args.cmd == 'land':
            ghstack.land.main(
                pull_request=args.pull_request,
                github=github,
                sh=sh,
                github_url=conf.github_url,
                remote_name=conf.remote_name,
            )
        elif args.cmd == 'action':
            ghstack.action.main(
                pull_request=args.pull_request,
                github=github,
                close=args.close,
            )
        elif args.cmd == 'checkout':
            ghstack.checkout.main(
                pull_request=args.pull_request,
                github=github,
                sh=sh,
                remote_name=conf.remote_name,
            )
        else:
            raise RuntimeError("Unrecognized command {}".format(args.cmd))


def create_shell(conf: ghstack.config.Config) -> ghstack.shell.Shell:
    import os
    import pathlib
    cwd = pathlib.Path(os.getcwd())
    candidates = [cwd] + list(pathlib.Path(os.getcwd()).parents)
    sapling_dotdir_candidates = [
        # @fb-only
        '.sl',
    ]
    for c in candidates:
        git_dir = c.joinpath('.git')
        if git_dir.is_dir():
            break
        for dotdir in sapling_dotdir_candidates:
            sapling_dir = c.joinpath(dotdir)
            if sapling_dir.is_dir():
                import ghstack.sapling_shell
                return ghstack.sapling_shell.SaplingShell(conf=conf)

    import ghstack.shell
    return ghstack.shell.Shell()


if __name__ == "__main__":
    main()
