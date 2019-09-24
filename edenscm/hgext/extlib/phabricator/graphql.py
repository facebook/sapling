# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# graphql.py
#
# A library function to call a phabricator graphql RPC.
# This replaces the Conduit methods

from __future__ import absolute_import

import json
import operator

from edenscm.mercurial import encoding, pycompat, util

from . import arcconfig, phabricator_graphql_client, phabricator_graphql_client_urllib


urlreq = util.urlreq


class ClientError(Exception):
    def __init__(self, code, msg):
        Exception.__init__(self, msg)
        self.code = code


class Client(object):
    def __init__(self, repodir=None, ca_bundle=None, repo=None):
        if not repodir:
            repodir = pycompat.getcwd()
        self._mock = "HG_ARC_CONDUIT_MOCK" in encoding.environ
        if self._mock:
            with open(encoding.environ["HG_ARC_CONDUIT_MOCK"], "r") as f:
                self._mocked_responses = json.load(f)
                # reverse since we want to use pop but still get items in
                # original order
                self._mocked_responses.reverse()

        self._host = None
        self._user = None
        self._cert = None
        self._oauth = None
        self._catslocation = None
        self._cats = None
        self.ca_bundle = ca_bundle or True
        self._applyarcconfig(
            arcconfig.loadforpath(repodir), repo.ui.config("phabricator", "arcrc_host")
        )
        if not self._mock:
            app_id = repo.ui.config("phabricator", "graphql_app_id")
            app_token = repo.ui.config("phabricator", "graphql_app_token")
            self._host = repo.ui.config("phabricator", "graphql_host")
            self._client = phabricator_graphql_client.PhabricatorGraphQLClient(
                phabricator_graphql_client_urllib.PhabricatorGraphQLClientRequests(),
                self._cert,
                self._oauth,
                self._cats,
                self._user,
                "phabricator",
                self._host,
                app_id,
                app_token,
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
                hostkeys = allhosts.keys()
                if len(hostkeys) > 0:
                    arcrchost = hostkeys[0]
                else:
                    self._raisearcrcerror()

        hostconfig = allhosts[arcrchost]

        self._user = hostconfig.get("user", None)
        self._cert = hostconfig.get("cert", None)
        self._oauth = hostconfig.get("oauth", None)
        self._catslocation = hostconfig.get("crypto_auth_tokens_location", None)
        if self._catslocation is not None:
            try:
                with open(self._catslocation, "r") as cryptoauthtokensfile:
                    cryptoauthtokensdict = json.load(cryptoauthtokensfile)
                    self._cats = cryptoauthtokensdict.get("crypto_auth_tokens")
            except Exception:
                pass

        if not self._user or (
            self._cert is None and self._oauth is None and self._cats is None
        ):
            self._raisearcrcerror()

    @classmethod
    def _raisearcrcerror(cls):
        raise arcconfig.ArcConfigError(
            "arcrc is missing user "
            "credentials. use "
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
                    latest_phabricator_version {
                      description
                      source_control_system
                      phabricator_version_properties {
                        edges {
                          node {
                            property_name
                            property_value
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
        """
        params = {"diffid": diffid}
        ret = self._client.query(timeout, query, params)
        return ret["data"]["phabricator_diff_query"][0]["results"]["nodes"][0][
            "latest_phabricator_version"
        ]

    def getrevisioninfo(self, timeout, *revision_numbers):
        rev_numbers = self._normalizerevisionnumbers(revision_numbers)
        if self._mock:
            ret = self._mocked_responses.pop()
        else:
            params = {"params": {"numbers": rev_numbers}}
            ret = self._client.query(timeout, self._getquery(), params)
        return self._processrevisioninfo(ret)

    def _getquery(self):
        return """
        query RevisionQuery(
          $params: [PhabricatorDiffQueryParams!]!
        ) {
          query: phabricator_diff_query(query_params: $params) {
            results {
              nodes {
                number
                diff_status_name
                latest_active_diff: latest_active_phabricator_version {
                  local_commit_info: phabricator_version_properties (
                    property_names: ["local:commits"]
                  ) {
                    nodes {
                      property_value
                    }
                  }
                }
                latest_publishable_draft_phabricator_version {
                  local_commit_info: phabricator_version_properties (
                    property_names: ["local:commits"]
                  ) {
                    nodes {
                      property_value
                    }
                  }
                }
                created_time
                updated_time
                is_landing
                differential_diffs: phabricator_versions {
                  count
                }
                signal_summary {
                  signals_status
                }
              }
            }
          }
        }
        """

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
        try:
            nodes = ret["data"]["query"][0]["results"]["nodes"]
            for node in nodes:
                info = {}
                infos[str(node["number"])] = info

                status = node["diff_status_name"]
                # GraphQL uses "Closed" but Conduit used "Committed" so let's
                # not change the naming
                if status == "Closed":
                    status = "Committed"
                info["status"] = status
                info["created"] = node["created_time"]
                info["updated"] = node["updated_time"]
                info["is_landing"] = node["is_landing"]

                info["signal_status"] = None
                if (
                    "signal_summary" in node
                    and "signals_status" in node["signal_summary"]
                ):
                    info["signal_status"] = (
                        node["signal_summary"]["signals_status"]
                        .title()
                        .replace("_", " ")
                    )

                active_diff = None
                if (
                    "latest_active_diff" in node
                    and node["latest_active_diff"] is not None
                ):
                    active_diff = node["latest_active_diff"]

                if (
                    "latest_publishable_draft_phabricator_version" in node
                    and node["latest_publishable_draft_phabricator_version"] is not None
                ):
                    active_diff = node["latest_publishable_draft_phabricator_version"]

                if active_diff is None:
                    continue

                info["count"] = node["differential_diffs"]["count"]

                localcommitnode = active_diff["local_commit_info"]["nodes"]
                if localcommitnode is not None and len(localcommitnode) == 1:
                    localcommits = json.loads(localcommitnode[0]["property_value"])

                    if not isinstance(localcommits, dict):
                        continue

                    localcommits = sorted(
                        localcommits.values(),
                        key=operator.itemgetter("time"),
                        reverse=True,
                    )
                    info["hash"] = localcommits[0].get("commit", None)

        except (AttributeError, KeyError, TypeError):
            raise ClientError(None, "Unexpected graphql response format")

        return infos
