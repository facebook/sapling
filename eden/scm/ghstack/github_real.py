import json
import logging
from urllib.request import urlopen, Request
from typing import Any, Final, Optional, Sequence, Tuple, Union

import ghstack.github


class RealGitHubEndpoint(ghstack.github.GitHubEndpoint):
    """
    A class representing a GitHub endpoint we can send queries to.
    It supports both GraphQL and REST interfaces.
    """

    # The URL of the GraphQL endpoint to connect to
    graphql_endpoint: str = 'https://api.{github_url}/graphql'

    # The base URL of the REST endpoint to connect to (all REST requests
    # will be subpaths of this URL)
    rest_endpoint: str = 'https://api.{github_url}'

    # The string OAuth token to authenticate to the GraphQL server with
    oauth_token: str

    # The URL of a proxy to use for these connections (for
    # Facebook users, this is typically 'http://fwdproxy:8080')
    proxy: Final[Optional[str]]

    # The certificate bundle to be used to verify the connection.
    # Passed to requests as 'verify'.
    verify: Optional[str]

    # Client side certificate to use when connecitng.
    # Passed to requests as 'cert'.
    cert: Optional[Union[str, Tuple[str, str]]]

    def __init__(self,
                 oauth_token: str,
                 github_url: str,
                 proxy: Optional[str] = None,
                 verify: Optional[str] = None,
                 cert: Optional[Union[str, Tuple[str, str]]] = None):
        self.oauth_token = oauth_token
        self.proxy = proxy
        self.github_url = github_url
        self.verify = verify
        self.cert = cert

    def push_hook(self, refName: Sequence[str]) -> None:
        pass

    def graphql_sync(self, query: str, **kwargs: Any) -> Any:
        headers = {}
        if self.oauth_token:
            headers['Authorization'] = 'bearer {}'.format(self.oauth_token)

        logging.debug("# POST {}".format(self.graphql_endpoint.format(github_url=self.github_url)))
        logging.debug("Request GraphQL query:\n{}".format(query))
        logging.debug("Request GraphQL variables:\n{}"
                      .format(json.dumps(kwargs, indent=1)))

        # TODO: Leverage self.verify and self.cert, if set.
        request = Request(
            self.graphql_endpoint.format(github_url=self.github_url),
            data=json.dumps({"query": query, "variables": kwargs}).encode('utf8'),
            headers=headers,
        )
        if self.proxy:
            request.set_proxy(self.proxy, 'http')
            request.set_proxy(self.proxy, 'https')

        with urlopen(request) as resp:
            logging.debug("Response status: {}".format(resp.status))

            body = resp.read()
            try:
                r = json.loads(body)
            except ValueError:
                logging.debug("Response body:\n{}".format(body))
                raise
            else:
                pretty_json = json.dumps(r, indent=1)
                logging.debug("Response JSON:\n{}".format(pretty_json))

        if 'errors' in r:
            pretty_json = json.dumps(r, indent=1)
            raise RuntimeError(pretty_json)

        return r

    def rest(self, method: str, path: str, **kwargs: Any) -> Any:
        headers = {
            'Authorization': 'token ' + self.oauth_token,
            'Content-Type': 'application/json',
            'User-Agent': 'ghstack',
            'Accept': 'application/vnd.github.v3+json',
        }

        url = self.rest_endpoint.format(github_url=self.github_url) + '/' + path
        logging.debug("# {} {}".format(method, url))
        logging.debug("Request body:\n{}".format(json.dumps(kwargs, indent=1)))

        # TODO: Leverage self.verify and self.cert, if set.
        request = Request(
            url,
            data=json.dumps(kwargs).encode('utf8') if kwargs is not None else None,
            headers=headers,
            method=method.upper(),
        )
        if self.proxy:
            request.set_proxy(self.proxy, 'http')
            request.set_proxy(self.proxy, 'https')

        with urlopen(request) as resp:
            status = resp.status
            logging.debug("Response status: {}".format(status))

            body = resp.read()
            try:
                r = json.loads(body)
            except ValueError:
                logging.debug("Response body:\n{}".format(body))
                raise

        if status == 404:
            raise RuntimeError("""\
GitHub raised a 404 error on the request for
{url}.
Usually, this doesn't actually mean the page doesn't exist; instead, it
usually means that you didn't configure your OAuth token with enough
permissions.  Please create a new OAuth token at
https://{github_url}/settings/tokens and DOUBLE CHECK that you checked
"public_repo" for permissions, and update ~/.ghstackrc with your new
value.
""".format(url=url, github_url=self.github_url))

        if 400 <= status and status < 600:
            pretty_json = json.dumps(r, indent=1)
            raise RuntimeError(pretty_json)

        return r
