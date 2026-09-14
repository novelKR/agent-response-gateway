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

The supplied [standalone management application](standalone-management.md) composes these adapters with explicit stores, read-only Web and optional Team access.

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

For package assembly, build committed source once into a new directory and share
those verified bytes across native targets. This export build requires Python
3.11+, Node 24.21.0 and npm 11.19.0; it does not require Cargo. It uses HEAD,
not uncommitted edits. Ordinary local development continues to use the commands above.

```sh
python3 -B scripts/web_assets.py .local/web-assets
python3 -B scripts/optional_package.py build --base .local/package-candidate --output .local/optional-candidate --web-assets .local/web-assets
```

Consumers verify source commit, HTTP/state contracts, complete asset inventory,
asset hashes and the exact reviewed notices against their source export. With
`--web-assets`, native assembly does not rebuild Web or require Node. Omitting
that option retains the independent local package build. Web build compatibility
on Windows remains part of full CI. Shared CI inputs are not release attestations.

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

<a id="appearance-and-shared-presentation"></a>
<a id="화면-표현과-공통-테마"></a>

## Appearance and shared presentation

The dashboard offers Light, Dark and System themes. With no saved selection it
uses Light. System follows the browser's color preference while selected.
Only language and theme preferences are stored locally; credentials and server
state are not persisted in browser storage. Restricted browser storage does not
prevent theme selection for the current page.

Both themes use shared semantic CSS tokens for surfaces, text, status, focus,
spacing and typography. The read-only views and authorization rules are the same
in either theme. Client and clock injection are internal presentation seams;
the production entry point always defaults to the real same-origin API client.
Disposing a view cancels its pending client requests and presentation listeners.


<a id="개발용-시나리오-canvas"></a>

## Development scenario canvas

The development-only DevDemo shares the production dashboard components, API
client parser and theme tokens. It is not a Gateway runtime or a public deployment.

```sh
npm ci --prefix management-web --ignore-scripts
npm run devdemo --prefix management-web
```

Use Node 24.21.0 and npm 11.19.0. Open the printed numeric loopback URL
(default port 43142); use `-- --port 43144` to select another available port.
Rust, Gateway configuration and credentials are not required for synthetic mode.
Stop the launcher with Ctrl+C. Only this entry point permits development HMR;
product and documentation previews continue to serve verified static files.

The separate control panel selects scenario, page, theme, language, viewport and
reset. The iframe has an actual 360, 768 or 1280 CSS-pixel viewport, or the available
width. Tables scroll within the view. The panel identifies synthetic data at all
times and disables pages outside the selected host's permissions.

Scenarios cover ready/stopped runtime, pending configuration, external changes,
different installed/selected/effective versions, unobserved/unsupported modules,
own Team usage, restricted Embedded views, empty/long/paginated lists, loading,
denied/expired sessions, connection errors, exact large/zero/unknown usage and
succeeded/failed/uncertain audit rows. Responses use a fixed clock and seed.
Loading retains the real client's 15-second timeout; reset starts it again.
Reset and scenario changes dispose the previous view and cancel its requests.
Presentation/scenario state is not persisted and no real model call is made.

Edit shared semantic CSS tokens or components to see HMR updates. The development
server restricts Host, Origin, WebSocket upgrades and files to the required Web
source/dependency directories. It does not serve private state or arbitrary
workspace files and provides no management API in synthetic mode.

Production builds reject development modules and fixture/HMR content. DevDemo is
excluded from product Web archives; the corresponding source archive retains its
development sources. Existing direct real-API fixture verification remains a
separate test. A successful synthetic view is not authentication or runtime
acceptance.


<a id="실제-api-fixture-개발-모드"></a>

## Actual API fixture development mode

To inspect the same UI through the actual management router, use the optional
fixture launcher:

```sh
npm run devdemo:fixture --prefix management-web
npm run devdemo:fixture --prefix management-web -- --preset usage
```

This mode also requires Rust 1.98.0. The launcher builds the locked
`web_fixture` example into the repository's `target/`, starts its API-only mode,
and owns its stdin lifetime. The default `all` preset permits state, usage and
audit reads; `usage` permits usage only. Both use the existing synthetic read key,
shown in the DevDemo panel, with normal API login/logout. Use only that test key.
The runtime fixture receives a minimal platform environment, not provider or
management credentials.

The panel identifies actual API transport with synthetic fixture state. Scenario
and fault injection controls are disabled; choose the preset in the launch
command. Page, theme, language, viewport and reset remain available. The shell and
canvas share theme tokens, and presentation choices remain temporary. Reset
recreates the view; it does not bypass or renew API authentication.

The HMR server forwards only the dashboard's read routes and session creation/
deletion to the fixture it started. It checks incoming Host/Origin before mapping
them to the fixture origin, forwards only the fixture's own session cookie, and
uses a direct local HTTP agent. Bearer headers are forwarded only for session
creation. It does not accept backend URLs or proxy management
mutations, continuation controls, model requests or arbitrary files. Existing
direct static-fixture tests separately verify the original API origin boundary.

Ctrl+C or supervised stdin closure stops both servers. Unexpected fixture exit
closes DevDemo with a failure; configuration, build or transport errors never
select synthetic fallback. Stop one launcher before changing mode (both default
to port 43142), or explicitly select another port. Each fixture owns a fresh
temporary audit store, and modes do not share authentication state.

The existing `web_fixture --assets ...` static workflow remains available.
`--api-only` and `--assets` are mutually exclusive. The same optional
`--preset all|usage` applies to either workflow. These are development fixture
options, not additions to the product management API.
