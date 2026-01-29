export const onRequestGet: PagesFunction<{ CLIENT_ID: string }> = async (context) => {
  const redirectUri = new URL('/_oauth/callback', context.request.url).toString();
  const githubAuthUrl = new URL('https://github.com/login/oauth/authorize');
  githubAuthUrl.searchParams.set('client_id', context.env.CLIENT_ID);
  githubAuthUrl.searchParams.set('redirect_uri', redirectUri);
  githubAuthUrl.searchParams.set('scope', 'user repo');

  return Response.redirect(githubAuthUrl.toString(), 302);
};
