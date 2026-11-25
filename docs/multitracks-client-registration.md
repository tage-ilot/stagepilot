# MultiTracks standalone MCP client registration

## Current external requirement

StagePilot MultiTracks Cues connects to `https://mcp.multitracks.com/mcp` as its own installed OAuth application. On July 16, 2026, the official protected-resource metadata named `https://account.multitracks.com/` as the authorization server and advertised scope `mcp`. Authorization metadata advertised authorization-code and refresh-token grants, S256 PKCE, token revocation, and the additional `openid`, `offline_access`, and `profile` scopes. It did not advertise a dynamic client-registration endpoint.

The utility therefore needs a client ID issued by MultiTracks. It does not reuse or impersonate the OAuth credentials documented for ChatGPT, Claude, or another product.

## Registration request

- Application: **StagePilot MultiTracks Cues**
- Application type: native/installed public client
- MCP resource: `https://mcp.multitracks.com/mcp`
- Flow: OAuth 2 authorization code with S256 PKCE and refresh tokens
- Redirect: loopback HTTP on `127.0.0.1`, dynamically allocated port, path `/oauth/callback`
- Requested scopes: `mcp offline_access openid profile` (subject to MultiTracks approval)
- Client authentication: public client with no secret preferred; a secret can be stored in Keychain/Credential Manager if MultiTracks explicitly requires and issues one
- Purpose: inspect a user-selected Playback setlist and safely create a channel-1, note-112, velocity-100 MIDI event at song start after explicit user approval

### Support message

> Hello MultiTracks Support,
>
> I maintain StagePilot, an open-source live-production application. We are building a standalone installed command-line client for the official MultiTracks MCP endpoint at https://mcp.multitracks.com/mcp. It uses OAuth authorization code with S256 PKCE, a dynamically allocated 127.0.0.1 loopback redirect ending in /oauth/callback, and refresh tokens. The requested scopes discovered from your metadata are mcp, offline_access, openid, and profile. The client performs read-only setlist/MIDI inspection by default and creates a single user-approved MIDI note event; it does not scrape the website or automate Playback’s UI. Your metadata does not currently advertise dynamic client registration. Could you issue a standalone native/public OAuth client ID for “StagePilot MultiTracks Cues” and confirm the permitted redirect URI pattern and scopes? If a client secret is required, please confirm that requirement for an installed application.

Submit this message through the official [MultiTracks contact form](https://www.multitracks.com/contact/). Choose the closest support/developer-integration category available and include a link to the StagePilot repository if the form permits it. Do not send StagePilot’s Planning Center credentials or any existing OAuth token.

After MultiTracks issues the client configuration, run `stagepilot-cues configure`, enter the issued values, and then run `stagepilot-cues auth login`.

