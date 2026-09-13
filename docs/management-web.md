<a id="브라우저-세션-경계"></a>
<a id="빌드와-소스-근거"></a>
<a id="조회-전용-관리-web"></a>
<a id="합성-브라우저-검증"></a>
<a id="화면과-관측-의미"></a>

# Read-only management Web

[English](management-web.md) | [한국어](ko/management-web.md)

`management-web/` is an optional, independently built Vue application for the
[management HTTP API](management-api.md). It displays runtime and configuration
metadata, extension inventory, usage and durable management operations. It creates
no Gateway process and adds no dependency to the default Gateway. A trusted host
must serve its verified static files and the management API at the same numeric
loopback origin. Concrete runtime and extension assembly has separate validation;
the synthetic browser fixture is not a product configuration.

## Views and observation semantics

The application uses `gateway-management-http/v1` and
`gateway-management-state/v1`. Navigation follows the authenticated subject's
allowed read operations. It reads runtime modules using
`gateway-runtime-status/v1` and extension modules using
`gateway-extension-status/v1`. Other modules retain their contract and observation
status in the overview. Unsupported and unobserved modules do not become empty
successful inventories.

API response time and module observation time are displayed separately. Failed
refreshes retain earlier observations with a warning. The next selected
configuration and effective configuration stay separate. Extension rows show
installed versions, next-start selections, effective selections and requested/granted
permissions; an unobserved effective selection is unknown. Codec inclusion does
not imply a resident process. No installation, removal, activation, configuration
change, process control or reconciliation buttons are provided.

Usage preserves null counters, partial and unobserved results, and unfinished
attempts. Counters larger than JavaScript's safe integer range retain their exact
digits as text. A browser lacking precise JSON reviver source support rejects
such a response explicitly. The view does not calculate charges or infer model
completion from an HTTP response. Operation history uses ascending journal cursors
and loads 20 records per page. Details show both the recorded state and the
current observed state, including uncertainty without rewriting past events.

The application refreshes visible active sessions every 15 seconds while the
first operation page is selected. Loading older history pauses automatic refresh;
manual Refresh returns to the first page. Each fetch has a 15-second deadline and
a 2-MiB response bound. There is no automatic mutation submission or retry.

## Browser session boundary

Only a dedicated read credential opens the dashboard. The input is cleared after
submission; credentials and returned data are not persisted to browser storage.
Only the language preference is stored locally. The management server supplies
a path-scoped HttpOnly, SameSite=Strict cookie and revalidates its authority.
Expired or revoked sessions return to authentication. Sign out removes the
current read session. Mutation credentials cannot create a browser session.

The client makes relative same-origin requests, rejects redirects, and has no
remote API fallback or built-in mock. Hosts must enforce exact Host and Origin,
serve a restrictive Content-Security-Policy, avoid wildcard CORS, and provide
`no-store`, `nosniff` and `no-referrer` headers as shown in the fixture. Static
files may be public on that loopback origin; all management data requires
authentication. Serving the static page does not authorize a data query.

## Build and source provenance

Use Node 24.21.0 and npm 11.19.0 for the independent build:

```sh
npm ci --prefix management-web --ignore-scripts
npm test --prefix management-web
npm run build --prefix management-web
node management-web/scripts/check-output.mjs
```

The committed lock reuses Vue 3.5.42, Vite 5.4.21 and the Vue plugin 5.2.4.
Node is not needed to serve the resulting `.local/management-web/dist/` files.
The documentation site remains a separate application. Its notice inventory
plugin is shared only during builds. Neither application runs Vite's development
server; the existing [development-server constraints](documentation.md#web-notices-and-development-server-constraints)
continue to apply.

The Web manifest records HTTP/state contract versions, source commit, dirty-tree
status, read-only scope and each asset SHA-256. The unchanged project license,
exact shipped package inventory and original notices accompany the output.
`node management-web/scripts/check-output.mjs COMMIT` requires clean provenance
for that exact commit. A local dirty preview is not an exact-source distribution
or an attestation. After reviewing changed license terms, originals and embedded
assets, `npm run build --prefix management-web -- --record-notices` records the
new inventory; recording hashes is not approval of commercial rights. Normal
builds and CI never stamp a review.

`npm run preview --prefix management-web` serves only verified static output at
`http://127.0.0.1:43141/dashboard/`. It provides no management API. Restore the
previous verified static artifact for a Web rollback; no audit, usage or
continuation storage is migrated or deleted.

## Synthetic browser verification

The test-only `web_fixture` Rust example serves the actual management router,
read-session authentication, a disposable SQLite journal and synthetic module
views with the built assets. It never launches a Gateway or a provider call.
It closes when stdin closes. Its fixed credential exists only in fixture source.

```sh
cargo build -p gateway-management-api --example web_fixture --locked
WEB_FIXTURE_BIN="$PWD/target/debug/examples/web_fixture" npm test --prefix management-web
cargo run -p gateway-management-api --example web_fixture --locked -- --assets .local/management-web/dist
```

For an interactive fixture run, open the printed numeric loopback URL, target
`gateway`, using `synthetic-browser-read-key-01234567890123456789`. These are
synthetic test values, not product credentials. Check both languages, keyboard
navigation and dialog focus, small screens, differing extension selections,
unknown usage counters, permission failures and logout. Fixture success is
separate from actual adapter assembly, hosted CI or consumer acceptance.
