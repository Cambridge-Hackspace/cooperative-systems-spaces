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

## Messaging members (the notification relay)

An app can send a member a message through CS:S (#226), such as "your print is
done". CS:S delivers it however that member has chosen, so the app never needs
their address, and never learns it.

1. **Register the app with `notify:members`.** It needs a secret: it acts as
   itself, through the client-credentials grant
   (`grant_type=client_credentials&scope=notify:members`).
2. **Declare the kinds of message it sends**, with labels members read:
   `PUT /api/notify/kinds` with
   `{"kinds": [{"kind": "print.done", "label": "Your print is done"}]}`. This
   replaces the app's whole set. A kind that is no longer declared is dropped,
   along with members' choices about it.
3. **Send**: `POST /api/notify/members/{sub}` with
   `{"kind", "title", "body"?, "link"?, "urgency"?: "normal"|"high", "dedupe_key"?}`.
   `sub` is the member's id from their sign-in.

The answer is `202` with the message's id, and it is the same whether the
member receives the message or not. Each part of a message is limited:

- **Title:** one line of plain text. It becomes the email subject.
- **Body:** plain text. Line breaks are kept.
- **Link:** an absolute http(s) URL, shown as text.
- **`dedupe_key`:** a repeated key returns the first message's id and sends
  nothing more.

**What a member gets.** Each member chooses under **Profile → Notifications
from apps**: per app, and per kind. Until they choose, only `high`-urgency
messages reach them. Mail goes only to a confirmed address. Email is the only
channel today.

**Limits.** `oidc.notify_per_member_per_hour` (30) and
`oidc.notify_per_app_per_hour` (1000). Past either, the answer is `429`.

**The record.** Every message is an audit event, `member_notified`. It holds the
app, the member, the kind and what became of the message (`delivered`,
`failed` or `suppressed`). It holds no address and no text.

## What it doesn't do

- **SAML.** CS:S signs members in through OpenID Connect only. (POP accepts SAML
  from other providers.)
- **Implicit or hybrid flows, or `plain` PKCE.** Only the authorization code
  flow, with S256.
- **Dynamic client registration.** An administrator registers each app.
