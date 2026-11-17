# CS:S as an OpenID Connect provider

CS:S can sign members in to other programs, the way Google or GitHub can
(#225). POP, the print tower, is the first: a member taps a card at the tower
or signs in on their phone, and POP learns who they are and which tools they
are trained on, without keeping accounts of its own.

It is off unless enabled.

## Turning it on

```toml
[oidc]
enabled = true
# The URL apps are told CS:S is at. Empty means site.site_url.
issuer = ""
```

Discovery is then served at `{issuer}/.well-known/openid-configuration`, and
an app configured with the issuer finds everything else from there.

The other settings and their defaults:

| Setting | Default | What it is |
| --- | --- | --- |
| `key_encryption_key` | empty | Hex 32-byte key that seals the signing keys at rest. Empty derives one from `auth.jwt_secret`, so changing that secret also ends every token signed before it changed. |
| `id_token_ttl_secs` | 3600 | How long an ID token is good for. |
| `access_token_ttl_secs` | 3600 | How long an access token is good for. |
| `refresh_token_ttl_days` | 30 | How long a refresh token is good for, if it isn't used first. |
| `code_ttl_secs` | 60 | How long an authorization code is good for. |
| `request_ttl_secs` | 600 | How long a sign-in may be left half-finished. |
| `key_rotation_days` | 90 | How often a new signing key replaces the current one. |

## Registering an app

**Admin → Apps**, with the `oauth.clients.manage` permission (granted to
`admin` by default). An app has:

- **Redirect URIs.** Absolute `https` URLs, or `http` on `localhost` for local
  tools. They are matched exactly, character for character.
- **Scopes** it may ask for:

  | Scope | What the app gets |
  | --- | --- |
  | `openid` | Who the member is: `sub`, their user UUID. Always included. |
  | `profile` | `name` and `preferred_username`. |
  | `email` | `email` and `email_verified`. |
  | `roles` | `roles`: the member's CS:S role names. |
  | `training:read` | `GET /api/oauth/me/tools`: each tool, and whether the member may use it. |
  | `notify:members` | The app acting as itself, to message members (#226). Only through the client-credentials grant, and only for an app with a secret. |

- **A secret, or not.** A server has one. An app running in a browser or on a
  phone can't keep one, and signs in with PKCE alone. The secret is shown once,
  when it is made. CS:S keeps only its argon2 hash.
- **"The space's own app".** Members aren't asked to agree first. Leave it off
  for anything the space doesn't run itself.

Disabling an app stops it at once: its tokens are refused, and it can't get
new ones. Removing it deletes everything it was issued.

## The flow

The authorization code flow, with PKCE (S256) required of every app:

1. The app sends the browser to `authorize`.
2. CS:S checks the app and its redirect URI, then hands the browser to its own
   page, `/oauth/consent/{id}`. CS:S's session cookie is `SameSite=Strict`, so a
   browser arriving from another site doesn't send it. CS:S's own page is
   same-site, so it does.
3. That page signs the member in if needed, asks them to agree if needed, and
   sends them back to the app with a code.
4. The app exchanges the code at `token`, with its PKCE verifier and, if it
   has one, its secret.

Logout works the same way: `end_session_endpoint` hands the browser to
CS:S's own page, which ends the session. It goes to `post_logout_redirect_uri`
only if that URI is registered to the app the `id_token_hint` names, and
otherwise to CS:S's home page.

## Tokens

- **ID tokens and access tokens** are RS256 JWTs, signed by the key in the
  JWKS whose `kid` they name. `sub` is the member's user UUID. It is never the
  email, because members change addresses and accounts are merged.
- **Access tokens** have `aud` `{issuer}/api`. They are accepted only by
  `userinfo` and `me/tools`, never as a CS:S session.
- **Refresh tokens** are opaque, and rotate: each use returns a new one. Using
  one twice ends every token that member holds with that app, because a
  replayed refresh token means it was copied.
- **A code is used once.** Presenting it a second time ends everything the
  first exchange issued.

## Keys

The signing key rotates every `key_rotation_days`, or at once from
**Admin → Apps → Rotate the signing key**. A retired key stays in the JWKS
until every token it signed has expired, so rotation never breaks a sign-in in
progress.

## Merged accounts

Merging account B into account A ends B's sign-ins in every app: B's codes,
refresh tokens and consents go with B. A's `sub` doesn't change.

## What it doesn't do

- **SAML.** CS:S signs members in through OpenID Connect only. (POP accepts SAML
  from other providers.)
- **Implicit or hybrid flows, or `plain` PKCE.** Only the authorization code
  flow, with S256.
- **Dynamic client registration.** An administrator registers each app.
