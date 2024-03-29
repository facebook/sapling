javascript: (function () {
  const localport = 3000;
  const currentUrl = window.location.href;
  const githubPrUrlPattern = /https:\/\/github\.com\/([^\/]+)\/([^\/]+)\/pull\/(\d+)/;
  const match = currentUrl.match(githubPrUrlPattern);

  if (match && match[1] && match[2] && match[3]) {
    const owner = match[1];
    const repo = match[2];
    const pullRequestNumber = match[3];
    const localBaseUrl = `http://localhost:${localport}/`;
    window.location.href = `${localBaseUrl}${owner}/${repo}/pull/${pullRequestNumber}`;
  } else {
    alert('You are not viewing a GitHub pull request.');
  }
})();
