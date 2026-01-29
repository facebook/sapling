interface GitHubTokenResponse {
  access_token?: string;
  error?: string;
  error_description?: string;
}

export const onRequestGet: PagesFunction<{ CLIENT_ID: string; CLIENT_SECRET: string }> = async (context) => {
  const url = new URL(context.request.url);
  const code = url.searchParams.get('code');

  if (!code) {
    return new Response('Missing code parameter', { status: 400 });
  }

  const tokenResponse = await fetch('https://github.com/login/oauth/access_token', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Accept': 'application/json',
    },
    body: JSON.stringify({
      client_id: context.env.CLIENT_ID,
      client_secret: context.env.CLIENT_SECRET,
      code,
    }),
  });

  const data: GitHubTokenResponse = await tokenResponse.json();

  if (data.error || !data.access_token) {
    const errorMsg = encodeURIComponent(data.error_description || data.error || 'Unknown error');
    return Response.redirect(`${url.origin}/?error=${errorMsg}`, 302);
  }

  // Redirect back to app with token in hash (not exposed to server logs)
  return Response.redirect(`${url.origin}/#token=${data.access_token}`, 302);
};
