# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os
import subprocess
import sys
import warnings
from typing import List, Optional, Set

from facebook.eden.ttypes import Glob, GlobParams

from . import subcmd as subcmd_mod, tabulate
from .cmd_util import get_eden_instance, require_checkout
from .config import EdenCheckout, EdenInstance
from .subcmd import Subcmd
from .util import get_eden_cli_cmd


prefetch_profile_cmd = subcmd_mod.Decorator()

# find the profile inside the given checkout and return a set of its contents.
def get_contents_for_profile(
    checkout: EdenCheckout, profile: str, silent: bool
) -> Set[str]:
    k_relative_profiles_location = "xplat/scm/prefetch_profiles/profiles"

    profile_path = checkout.path / k_relative_profiles_location / profile

    if not profile_path.is_file():
        if not silent:
            print(f"Profile {profile} not found for checkout {checkout.path}.")
        return set()

    with open(profile_path) as f:
        return {pat.strip() for pat in f.readlines()}


# Function to actually cause the prefetch, can be called on a background process
# or in the main process.
# Only print here if silent is False, as that could send messages randomly to
# stdout.
def make_prefetch_request(
    checkout: EdenCheckout,
    instance: EdenInstance,
    all_profile_contents: Set[str],
    enable_prefetch: bool,
    silent: bool,
) -> Optional[Glob]:
    with instance.get_thrift_client_legacy() as client:
        return client.globFiles(
            GlobParams(
                mountPoint=bytes(checkout.path),
                globs=list(all_profile_contents),
                includeDotfiles=False,
                prefetchFiles=enable_prefetch,
                suppressFileList=silent,
            )
        )


# prefetch all of the files specified by a profile in the given checkout
def prefetch_profiles(
    checkout: EdenCheckout,
    instance: EdenInstance,
    profiles: List[str],
    run_in_foreground: bool,
    enable_prefetch: bool,
    silent: bool,
) -> Optional[Glob]:

    # if we are running in the foreground, skip creating a new process to
    # run in, just run it here.
    if run_in_foreground:
        all_profile_contents = set()

        for profile in profiles:
            all_profile_contents |= get_contents_for_profile(checkout, profile, silent)

        return make_prefetch_request(
            checkout=checkout,
            instance=instance,
            all_profile_contents=all_profile_contents,
            enable_prefetch=enable_prefetch,
            silent=silent,
        )
    # if we are running in the background, create a copy of the fetch command
    # but in the foreground.
    else:
        # note that we intentionally skip the verbose flag, since this is
        # running in the background there is no point to printing, eventually
        # we might write to a log at which point we would want to forward
        # the verbose flag
        fetch_sub_command = get_eden_cli_cmd() + [
            "prefetch_profile",
            "fetch",
            "--checkout",
            str(checkout.path),
            # Since we have already backgrounded, the background
            # process should run the fetch in the foreground.
            "--foreground",
        ]

        fetch_sub_command += ["--profile-names"] + profiles

        # we need to say if we are not suppose to prefetch as it is the
        # default to enable_prefetching
        if not enable_prefetch:
            fetch_sub_command += ["--skip-prefetch"]

        creation_flags = 0
        if sys.platform == "win32":
            # TODO add subprocess.DETACHED_PROCESS only available in python 3.7+
            # on windows, currently only 3.6 avialable
            creation_flags |= subprocess.CREATE_NEW_PROCESS_GROUP

        # Note that we can not just try except to catch warnings, because
        # warnings do not raise errors so the except would not catch them.
        # We would have to turn warnings into errors with
        # `warnings.filterwarnings('error')` and then we could catch them with
        # try except, but this is the more idomatic way to catch warnings.
        with warnings.catch_warnings(record=True):
            subprocess.Popen(
                fetch_sub_command,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                creationflags=creation_flags,
            )
            return None


@prefetch_profile_cmd("record", "Start recording fetched file paths.")
class RecordProfileCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        with instance.get_thrift_client_legacy() as client:
            client.startRecordingBackingStoreFetch()
        return 0


@prefetch_profile_cmd(
    "finish",
    "Stop recording fetched file paths and save previously"
    " collected fetched file paths in the output prefetch profile",
)
class FinishProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--output-path",
            nargs="?",
            help="The output path to store the prefetch profile",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        with instance.get_thrift_client_legacy() as client:
            files = client.stopRecordingBackingStoreFetch()
            output_path = (
                args.output_path
                if args.output_path
                else os.path.abspath("prefetch_profile.txt")
            )
            with open(output_path, "w") as f:
                for path in sorted(files.fetchedFilePaths["HgQueuedBackingStore"]):
                    f.write(os.fsdecode(path))
                    f.write("\n")
        return 0


@prefetch_profile_cmd(
    "list", "List all of the currenly activated prefetch profiles for a checkout."
)
class ListProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--checkout",
            help="The checkout for which you want to see all the profiles this profile.",
            default=None,
        )

    def run(self, args: argparse.Namespace) -> int:
        checkout = args.checkout

        instance, checkout, _rel_path = require_checkout(args, checkout)

        profiles = sorted(checkout.get_config().active_prefetch_profiles)

        columns = ["Name"]
        data = [{"Name": name} for name in profiles]

        print(tabulate.tabulate(columns, data))

        return 0


@prefetch_profile_cmd(
    "activate",
    "Tell EdenFS to smart prefetch the files specified by the prefetch profile."
    " (Eden will prefetch the files in this profile immediately, when checking "
    " out a new commit and for some commits on pull).",
)
class ActivateProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("profile_name", help="Profile to activate.")
        parser.add_argument(
            "--checkout",
            help="The checkout for which you want to activate this profile.",
            default=None,
        )
        parser.add_argument(
            "--verbose",
            help="Print extra info including warnings and the names of the "
            "matching files to fetch.",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--skip-prefetch",
            help="Still activate the profile, but do not prefetch profiles. "
            "This will still list the names of matching files for the profile "
            "when the verbose flag is also used",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--foreground",
            help="Run the prefetch in the main thread rather than in the"
            " background. Normally this command will return once the prefetched"
            " has been kicked off, but when this flag is used it to block until"
            " all of the files are prefetched.",
            default=False,
            action="store_true",
        )

    def run(self, args: argparse.Namespace) -> int:
        checkout = args.checkout

        instance, checkout, _rel_path = require_checkout(args, checkout)

        activation_result = checkout.activate_profile(args.profile_name)

        # error in activation, no point in continuing, so exit early
        if activation_result:
            return activation_result

        if not args.skip_prefetch:
            result = prefetch_profiles(
                checkout,
                instance,
                [args.profile_name],
                run_in_foreground=args.foreground,
                enable_prefetch=True,
                silent=not args.verbose,
            )
            if args.verbose and result is not None:
                for name in result.matchingFiles:
                    print(os.fsdecode(name))

        return 0


@prefetch_profile_cmd(
    "deactivate",
    "Tell EdenFS to STOP smart prefetching the files specified by the prefetch"
    " profile.",
)
class DeactivateProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("profile_name", help="Profile to activate.")
        parser.add_argument(
            "--checkout",
            help="The checkout for which you want to activate this profile.",
            default=None,
        )

    def run(self, args: argparse.Namespace) -> int:
        checkout = args.checkout

        instance, checkout, _rel_path = require_checkout(args, checkout)

        return checkout.deactivate_profile(args.profile_name)


@prefetch_profile_cmd(
    "fetch",
    "Prefetch all the active prefetch profiles or specified prefetch profiles. "
    "This is intended for use in after checkout and pull.",
)
class FetchProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--checkout",
            help="The checkout for which the profiles should be fetched.",
            default=None,
        )
        parser.add_argument(
            "--verbose",
            help="Print extra info including warnings and the names of the "
            "matching files to fetch. Note that the matching files fetched"
            "will not be printed when the foreground flag is passed.",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--skip-prefetch",
            help="Do not prefetch profiles only find all the files that match "
            "them. This will still list the names of matching files when the "
            "verbose flag is also used",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--foreground",
            help="Run the prefetch in the main thread rather than in the"
            " background. Normally this command will return once the prefetched"
            " has been kicked off, but when this flag is used it to block until"
            " all of the files are prefetched.",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--profile-names",
            nargs="*",
            help="Fetch only these named profiles instead of the active set of "
            "profiles.",
            default=None,
        )

    def run(self, args: argparse.Namespace) -> int:
        if sys.platform == "win32":
            # TODO(kmancini) prefetch profiles is not supported on windows yet
            return 0

        checkout = args.checkout

        instance, checkout, _rel_path = require_checkout(args, checkout)

        if args.profile_names is not None:
            profiles_to_fetch = args.profile_names
        else:
            profiles_to_fetch = checkout.get_config().active_prefetch_profiles

        if not profiles_to_fetch:
            if args.verbose:
                print("No profiles to fetch.")
            return 0

        result = prefetch_profiles(
            checkout,
            instance,
            profiles_to_fetch,
            run_in_foreground=args.foreground,
            enable_prefetch=not args.skip_prefetch,
            silent=not args.verbose,
        )

        if args.verbose and result is not None:
            for name in result.matchingFiles:
                print(os.fsdecode(name))
        return 0


class PrefetchProfileCmd(Subcmd):
    NAME = "prefetch_profile"
    HELP = "Create, manage, and use Prefetch Profiles. This command is "
    " primarily for use in automation."

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        self.add_subcommands(parser, prefetch_profile_cmd.commands)

    def run(self, args: argparse.Namespace) -> int:
        return 0
