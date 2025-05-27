# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# graphql.py
#
# A library function to call a phabricator graphql RPC.
# This replaces the Conduit methods


import os

from typing import Optional

from sapling import encoding, error, json, util
from sapling.i18n import _
from sapling.node import bin, hex

from . import arcconfig, phabricator_graphql_client, phabricator_graphql_client_urllib


urlreq = util.urlreq

GLOBAL_REV_TYPE = "GLOBAL_REV"


class ClientError(Exception):
    def __init__(self, code, msg):
        Exception.__init__(self, msg)
        self.code = code


class GraphQLConfigError(Exception):
    pass


class Client:
    def __init__(self, repodir=None, repo=None, ui=None):
        if repo is not None:
            if repodir is None:
                repodir = repo.root
            ui = ui or repo.ui

        if ui is None:
            raise error.ProgrammingError("either repo or ui needs to be provided")

        if not repodir:
            repodir = os.getcwd()
        self._mock = "HG_ARC_CONDUIT_MOCK" in encoding.environ
        if self._mock:
            with open(encoding.environ["HG_ARC_CONDUIT_MOCK"], "r") as f:
                self._mocked_responses = json.load(f)
                # reverse since we want to use pop but still get items in
                # original order
                self._mocked_responses.reverse()

        self._host = None
        self._user = None
        self._oauth = None
        self._catslocation = None
        self._cats = None
        self._applyarcconfig(
            arcconfig.loadforpath(repodir), ui.config("phabricator", "arcrc_host")
        )
        if not self._mock:
            app_id = ui.config("phabricator", "graphql_app_id")
            self._host = ui.config("phabricator", "graphql_host")
            if app_id is None or self._host is None:
                raise GraphQLConfigError(
                    "GraphQL unavailable because of missing configuration"
                )

            # phabricator.use-unix-socket is escape hatch in case something breaks.
            unix_socket_path = ui.configbool(
                "phabricator", "use-unix-socket", default=True
            ) and ui.config("auth_proxy", "unix_socket_path")

            self._client = phabricator_graphql_client.PhabricatorGraphQLClient(
                phabricator_graphql_client_urllib.PhabricatorGraphQLClientRequests(
                    unix_socket_proxy=unix_socket_path, ui=ui
                ),
                app_id if unix_socket_path else None,
                self._oauth,
                self._cats,
                self._host,
            )

    def _applyarcconfig(self, config, defaultarcrchost):
        arcrchost = config.get("graphql_uri", None)
        if "OVERRIDE_GRAPHQL_URI" in encoding.environ:
            arcrchost = encoding.environ["OVERRIDE_GRAPHQL_URI"]

        if "hosts" not in config:
            self._raisearcrcerror()

        allhosts = config["hosts"]

        if arcrchost not in allhosts:
            if defaultarcrchost in allhosts:
                arcrchost = defaultarcrchost
            else:
                # pick the first credential blob in hosts
                hostkeys = list(allhosts.keys())
                if len(hostkeys) > 0:
                    arcrchost = hostkeys[0]
                else:
                    self._raisearcrcerror()

        hostconfig = allhosts[arcrchost]

        self._user = hostconfig.get("user", None)
        self._oauth = hostconfig.get("oauth", None)
        self._catslocation = hostconfig.get("crypto_auth_tokens_location", None)
        if self._catslocation is not None:
            try:
                with open(self._catslocation, "r") as cryptoauthtokensfile:
                    cryptoauthtokensdict = json.load(cryptoauthtokensfile)
                    self._cats = cryptoauthtokensdict.get("crypto_auth_tokens")
            except Exception:
                pass

        if not self._user or (self._oauth is None and self._cats is None):
            self._raisearcrcerror()

    @classmethod
    def _raisearcrcerror(cls):
        raise arcconfig.ArcRcMissingCredentials(
            "arcrc is missing user "
            "credentials. Use "
            '"jf authenticate" to fix, '
            "or ensure you are prepping your arcrc properly."
        )

    def _normalizerevisionnumbers(self, *revision_numbers):
        rev_numbers = []
        if isinstance(revision_numbers, str):
            return [int(revision_numbers)]
        for r in revision_numbers:
            if isinstance(r, list) or isinstance(r, tuple):
                for rr in r:
                    rev_numbers.extend(rr)
            else:
                rev_numbers.append(int(r))
        return [int(x) for x in rev_numbers]

    def getdifflatestversion(self, timeout, diffid):
        query = """
            query DiffLastVersionDescriptionQuery($diffid: String!){
              phabricator_diff_query(query_params: {
                numbers: [$diffid]
              }) {
                results {
                    nodes {
                    latest_associated_phabricator_version_regardless_of_viewer {
                      description
                      repository {
                          scm_name
                      }
                      source_control_system
                      commit_hash_best_effort
                    }
                    phabricator_diff_commit {
                      nodes {
                        repository {
                          scm_name
                        }
                        commit_identifier
                      }
                    }
                  }
                }
              }
            }
        """
        params = {"diffid": diffid}
        ret = self._client.query(timeout, query, params)

        try:
            latest: Optional[dict] = ret["data"]["phabricator_diff_query"][0][
                "results"
            ]["nodes"][0]["latest_associated_phabricator_version_regardless_of_viewer"]

            if latest is None:
                raise ClientError(
                    None, _("D%s does not have any commits associated with it") % diffid
                )
        except (KeyError, IndexError):
            raise ClientError(
                None,
                _(
                    "Failed to get commit hash via Phabricator for D%s. GraphQL response:\n  %s"
                )
                % (diffid, json.dumps(ret)),
            )

        # Massage commits into {repo_name => commit_hash}
        commits = ret["data"]["phabricator_diff_query"][0]["results"]["nodes"][0][
            "phabricator_diff_commit"
        ]["nodes"]
        latest["commits"] = {
            commit["repository"]["scm_name"]: commit["commit_identifier"]
            for commit in commits
        }

        return latest

    def getnodes(self, repo, diffids, diff_status, timeout=10):
        """Get nodes for diffids for a list of diff status. Return {diffid: node}, {diffid: set(node)}"""
        if not diffids:
            return {}, {}, {}
        if self._mock:
            ret = self._mocked_responses.pop()
        else:
            query = """
                query DiffToCommitQuery($diffids: [String!]!){
                    phabricator_diff_query(query_params: {
                        numbers: $diffids
                    }) {
                        results {
                            nodes {
                                number
                                diff_status_name
                                phabricator_versions {
                                    nodes {
                                        local_commits {
                                            primary_commit {
                                                commit_identifier
                                            }
                                        }
                                    }
                                }
                                phabricator_diff_commit {
                                    nodes {
                                        commit_identifier
                                    }
                                }
                            }
                        }
                    }
                }
                """
            params = {"diffids": diffids}
            ret = self._client.query(timeout, query, params)
            # Example result:
            # { "data": {
            #     "phabricator_diff_query": [
            #       { "results": {"nodes": [{
            #               "number": 123,
            #               "diff_status_name": "Closed",
            #               "phabricator_versions": {
            #                 "nodes": [
            #                   {"local_commits": [{"primary_commit": {"commit_identifier": "d131c2d7408acf233a4b2db04382005434346421"}}]},
            #                   {"local_commits": [{"primary_commit": {"commit_identifier": "a421db7622bf0c454ab19479f166fd4a3a4a41f5"}}]},
            #                   {"local_commits": []}]},
            #               "phabricator_diff_commit": {
            #                 "nodes": [
            #                   { "commit_identifier": "9396e4a63208eb034b8b9cca909f9914cb2fbe85" } ] } } ] } } ] } }
        return self._getnodes(repo, ret, diff_status)

    def _getnodes(self, repo, ret, diff_status_list):
        difftolocalcommits = {}  # {str: set(node)}
        diffidentifiers = {}
        difftostatus = {}

        try:
            diffnodes = ret["data"]["phabricator_diff_query"][0]["results"]["nodes"]
        except (KeyError, IndexError):
            raise ClientError(
                None,
                _("Failed to get diff info via Phabricator. GraphQL response: %s")
                % json.dumps(ret),
            )

        for result in diffnodes:
            try:
                diffid = "%s" % result["number"]
                _status = result["diff_status_name"]
                if _status in diff_status_list:
                    difftostatus[diffid] = _status
                    nodes = result["phabricator_diff_commit"]["nodes"]
                    for n in nodes:
                        diffidentifiers[n["commit_identifier"]] = diffid

                    allversionnodes = result["phabricator_versions"]["nodes"]
                    for version in allversionnodes:
                        versioncommits = version["local_commits"]
                        for commit in versioncommits:
                            difftolocalcommits.setdefault(diffid, set()).add(
                                bin(commit["primary_commit"]["commit_identifier"])
                            )
            except (KeyError, IndexError, TypeError):
                # Not fatal.
                continue
        difftonode = {}
        maybehash = [bin(i) for i in diffidentifiers if len(i) == 40]
        # Batch up node existence checks using filternodes() in case
        # they trigger a network operation.
        for hashident in repo.changelog.filternodes(maybehash):
            difftonode[diffidentifiers.pop(hex(hashident))] = hashident

        difftoglobalrev = {}
        for identifier, diffid in diffidentifiers.items():
            # commit_identifier could be svn revision numbers, ignore
            # them.
            if identifier.isdigit():
                # This is probably a globalrev.
                difftoglobalrev[diffid] = identifier

        # Translate global revs to nodes.
        if difftoglobalrev:
            totranslate = [
                globalrev
                for diffid, globalrev in difftoglobalrev.items()
                if diffid not in difftonode
            ]
            globalrevtonode = {}
            if totranslate:
                if (
                    repo.ui.configbool("globalrevs", "edenapilookup")
                    and repo.nullableedenapi is not None
                ):
                    for translation in repo.edenapi.committranslateids(
                        [{"Globalrev": int(globalrev)} for globalrev in totranslate],
                        "Hg",
                    ):
                        globalrev = str(translation["commit"]["Globalrev"])
                        hgnode = translation["translated"]["Hg"]
                        globalrevtonode[globalrev] = hgnode
                    totranslate = [
                        globalrev
                        for globalrev in totranslate
                        if globalrev not in globalrevtonode
                    ]
                    if totranslate:
                        repo.ui.develwarn(
                            "Falling back to SCMQuery for globalrev lookup for %s\n"
                            % totranslate
                        )
            if totranslate:
                globalrevtonode.update(
                    self.getmirroredrevmap(repo, totranslate, GLOBAL_REV_TYPE, "hg")
                )
            if globalrevtonode:
                for diffid, globalrev in difftoglobalrev.items():
                    node = globalrevtonode.get(globalrev)
                    if node:
                        difftonode[diffid] = node
        return difftonode, difftolocalcommits, difftostatus

    def getrevisioninfo(self, timeout, signalstatus, *revision_numbers):
        rev_numbers = self._normalizerevisionnumbers(revision_numbers)
        if self._mock:
            ret = self._mocked_responses.pop()
        else:
            params = {"params": {"numbers": rev_numbers}}
            ret = self._client.query(timeout, self._getquery(signalstatus), params)
        return self._processrevisioninfo(ret)

    def graphqlquery(self, query, variables, timeout=60_000):
        if self._mock:
            return self._mocked_responses.pop()
        return self._client.query(timeout, query, variables)

    def _getquery(self, signalstatus):
        signalquery = ""

        if signalstatus:
            signalquery = """
                signal_summary {
                  signals_status
                }"""

        return (
            """
        query RevisionQuery(
          $params: [PhabricatorDiffQueryParams!]!
        ) {
          query: phabricator_diff_query(query_params: $params) {
            results {
              nodes {
                number
                diff_status_name
                latest_active_phabricator_version {
                  commit_hash_best_effort
                }
                latest_publishable_draft_phabricator_version {
                  commit_hash_best_effort
                }
                created_time
                updated_time
                is_landing
                land_job_status
                needs_final_review_status
                unpublished_phabricator_versions {
                  phabricator_version_migration {
                    ordinal_label {
                      abbreviated
                    }
                    commit_hash_best_effort
                  }
                }
                phabricator_versions {
                  nodes {
                    ordinal_label {
                      abbreviated
                    }
                    commit_hash_best_effort
                  }
                }
                %s
              }
            }
          }
        }
        """
            % signalquery
        )

    def _processrevisioninfo(self, ret):
        try:
            errormsg = None
            if "error" in ret:
                errormsg = ret["error"]
            if "errors" in ret:
                errormsg = ret["errors"][0]["message"]
            if errormsg is not None:
                raise ClientError(None, errormsg)
        except (KeyError, TypeError):
            pass

        infos = {}
        diff_number_str = None  # for error message
        try:
            nodes = ret["data"]["query"][0]["results"]["nodes"]
            for node in nodes:
                info = {}
                diff_number_str = str(node["number"])
                infos[diff_number_str] = info

                status = node["diff_status_name"]
                # GraphQL uses "Closed" but Conduit used "Committed" so let's
                # not change the naming
                if status == "Closed":
                    status = "Committed"
                info["status"] = status
                info["created"] = node["created_time"]
                info["updated"] = node["updated_time"]
                info["is_landing"] = node["is_landing"]
                info["land_job_status"] = node["land_job_status"]
                info["needs_final_review_status"] = node["needs_final_review_status"]

                info["signal_status"] = None
                if (
                    # signal_summary can be:
                    # 1. missing; 2. present, "None" (ex. D17868094)
                    node.get("signal_summary")
                    and "signals_status" in node["signal_summary"]
                ):
                    info["signal_status"] = (
                        node["signal_summary"]["signals_status"]
                        .title()
                        .replace("_", " ")
                    )

                alldiffversions = {}
                phabversions = node.get("phabricator_versions", {}).get("nodes", [])
                phabdraftversions = node.get("unpublished_phabricator_versions", [])
                for version in phabversions + phabdraftversions:
                    if "phabricator_version_migration" in version:
                        version = version["phabricator_version_migration"]
                    name = version.get("ordinal_label", {}).get("abbreviated")
                    vhash = version.get("commit_hash_best_effort")
                    if name and vhash:
                        alldiffversions[vhash] = name
                info["diff_versions"] = alldiffversions

                active_version = node.get(
                    "latest_publishable_draft_phabricator_version"
                )
                if active_version is None:
                    active_version = node.get("latest_active_phabricator_version", {})
                if active_version is not None:
                    commit_hash = active_version.get("commit_hash_best_effort")
                    if commit_hash is not None:
                        info["hash"] = commit_hash

        except (AttributeError, KeyError, TypeError):
            if diff_number_str is not None:
                msg = _("Unexpected graphql response format for D%s") % diff_number_str
            else:
                msg = _("Unexpected graphql response format")
            raise ClientError(None, msg)

        return infos

    def getmirroredrev(self, fromrepo, fromtype, torepo, totype, rev, timeout=15):
        """Transale a single rev to other repo/type"""
        query = self._getmirroredrevsquery()
        params = {
            "params": {
                "caller_info": "ext.exlib.phabricator.getmirroredrev",
                "from_repo": fromrepo,
                "from_scm_type": fromtype,
                "to_repo": torepo,
                "to_scm_type": totype,
                "revs": [rev],
            }
        }
        ret = self._client.query(timeout, query, json.dumps(params))
        self._raise_errors(ret)
        for pair in ret["data"]["query"]["rev_map"]:
            if pair["from_rev"] == rev:
                return pair["to_rev"]
        return ""

    def getmirroredrevmap(self, repo, nodes, fromtype, totype, timeout=15):
        """Return a mapping {node: node}

        Example:

            getmirroredrevmap(repo, [gitnode1, gitnode2],"git", "hg")
            # => {gitnode1: hgnode1, gitnode2: hgnode2}
        """
        reponame = repo.ui.config("fbscmquery", "reponame")
        if not reponame:
            return {}

        fromenc, fromdec = _getencodedecodefromcommittype(fromtype)
        _toenc, todec = _getencodedecodefromcommittype(totype)

        query = self._getmirroredrevsquery()
        params = {
            "params": {
                "caller_info": "ext.exlib.phabricator.getmirroredrevmap",
                "from_repo": reponame,
                "from_scm_type": fromtype,
                "to_repo": reponame,
                "to_scm_type": totype,
                "revs": list(map(fromenc, nodes)),
            }
        }
        ret = self._client.query(timeout, query, json.dumps(params))
        self._raise_errors(ret)
        result = {}
        for pair in ret["data"]["query"]["rev_map"]:
            result[fromdec(pair["from_rev"])] = todec(pair["to_rev"])
        return result

    def _getmirroredrevsquery(self):
        return """
            query GetMirroredRevs(
                $params: SCMQueryGetMirroredRevsParams!
            ) {
                query: scmquery_service_get_mirrored_revs(params: $params) {
                    rev_map {
                        from_rev,
                        to_rev
                    }
                }
            }
        """

    def scmquery_log(
        self,
        repo,
        scm_type,
        rev,
        file_paths=None,
        number=None,
        skip=None,
        exclude_rev_and_ancestors=None,
        before_timestamp=None,
        after_timestamp=None,
        timeout=10,
        use_mutable_history=False,
    ):
        """List commits from the repo meeting given criteria.

        Returns list of hashes.
        """
        query = """
            query ScmQueryLogV2(
                $params: SCMQueryServiceLogParams!
            ) {
                query: scmquery_service_log(params: $params) {
                    hash,
                }
            }
        """
        params = {
            "params": {
                "caller_info": "ext.extlib.phabricator.graphql.scmquery_log",
                "repo": repo,
                "scm_type": scm_type,
                "rev": rev,
                "file_paths": file_paths,
                "number": number,
                "skip": skip,
                "exclude_rev_and_ancestors": exclude_rev_and_ancestors,
                "before_timestamp": before_timestamp,
                "after_timestamp": after_timestamp,
                "follow_mutable_file_history": use_mutable_history,
            }
        }
        ret = self._client.query(timeout, query, json.dumps(params))
        self._raise_errors(ret)
        return ret["data"]["query"]

    def get_username(self, unixname=None, timeout=10) -> str:
        """Get a string suitable for ui.username, like "Foo bar <foobar@example.com>"."""
        if unixname is None:
            unixname = os.getenv("USER") or os.getenv("USERNAME")
        if not unixname:
            raise error.Abort(_("unknown unixname"))
        query = "query($u: String!) { intern_user_for_unixname(unixname: $u) { access_name email } }"
        params = {"u": unixname}
        # {'data': {'intern_user_for_unixname': {'access_name': 'Name', 'email': 'foo@example.com'}}}
        ret = self._client.query(timeout, query, json.dumps(params))
        self._raise_errors(ret)
        data = ret["data"]["intern_user_for_unixname"]
        if not data:
            raise error.Abort(_("no internal user for unixname '%s'") % unixname)
        username = f"{data['access_name']} <{data['email']}>"
        return username

    def _raise_errors(self, response):
        try:
            errormsg = None
            if "error" in response:
                errormsg = response["error"]
            if "errors" in response:
                errormsg = response["errors"][0]["message"]
            if errormsg is not None:
                raise ClientError(None, errormsg)
        except (KeyError, TypeError):
            pass


def _getencodedecodefromcommittype(committype):
    if committype == GLOBAL_REV_TYPE:
        encode = str

        def decode(x):
            return x

    else:
        # GraphQL wants hex, not bin
        encode = hex
        decode = bin
    return encode, decode
