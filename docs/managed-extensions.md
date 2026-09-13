<a id="검증과-복구"></a>
<a id="동시-변경과-증거"></a>
<a id="로컬-확장-관리의-영속-감사"></a>
<a id="설치선택실제-적용-상태"></a>
<a id="저장소와-원본의-소유권"></a>

# Audited local extension management

[English](managed-extensions.md) | [한국어](ko/managed-extensions.md)

The optional `gateway-management-extensions` library connects existing native
extension and profile-pack managers to the durable management journal. It starts
no gateway, listener or resident worker. HTTP and dashboard composition are
separate. The default gateway does not depend on this package.

## Store and source ownership

A trusted host registers one target, an existing package store, a private evidence
directory and named local package sources pinned by package SHA-256. Native
management also registers the absolute Python interpreter and manager script with
both file hashes. Python 3.11+ and Linux/macOS are required for native management;
profile packs use the existing portable Rust implementation. Unsupported native
platforms reject before creating adapter state.

`Manager::initialize` creates a new `gateway-extension-manager/v1` marker and
refuses overwrite. `Manager::open` requires that schema. The evidence directory
has a single owner lease; Windows privacy is a host ACL responsibility. Package
stores are explicitly prepared by the host using the existing manager or documented
store layout. No installation source is downloaded or discovered. HTTP callers
must use registered source IDs, not interpreter, shell or filesystem paths.

Status and source inspection are trusted-host read methods. Hosts must authorize
those reads. Mutations go through `Manager::bind` and the management journal,
which checks fresh actor grants, command digest and the expected store snapshot.
The adapter never reads provider, team or management credential values.

## Installed, selected and effective state

Inventory reports installed exact versions, verification results and the existing
activation snapshot separately. Damaged packages remain visible with failed
verification; an interrupted or unrecognized installation artifact fails inventory
explicitly instead of disappearing. A scan is bounded to 128 installed packages
and 1024 directory entries. Package notice text and model payloads are not copied
into profile inventory.

A trusted runtime owner may supply `EffectiveSelection`, including its instance,
observation time, configuration/execution digests and exact selected packages.
Without that observation, effective state is unknown. These entries mean inclusion
in the runtime configuration, not a permanently running codec process. Codec v2
continues to execute per request. Changing selection does not update a supplied
runtime observation or claim that a restart happened.

| Operation | Native extension | Profile pack |
|---|---|---|
| Install | Exact local manifest and every file digest checked | Exact canonical single-file digest checked |
| Enable | Exact requested permissions and optional registered recorder binding required | No executable permissions; exact installed selection required |
| Select version | Explicit version/digest selection with fresh grants; earlier versions remain installed | Disable the prior binding, then enable the exact replacement |
| Disable | Removes the next-start activation binding and preserves package/state/usage bytes | Removes activation and preserves all package bytes |
| Remove package or data | Unsupported | Unsupported |

Managed native enable and version selection have separate action grants: enable
cannot replace an active version, and selection requires an existing binding.
The legacy CLI retains its explicit enable-with-replacement behavior.

`removal_supported` is false. Unsupported operations fail rather than returning
success. There is no uninstall, data deletion, remote search/download, update
service, gateway restart or model retry in this adapter. Installation and
activation never execute package code. Recorder-to-observer replacement clears
the recorder activation binding while retaining its usage data.

## Concurrency and evidence

Native guarded operations check generation and inventory digest inside the
existing Python mutation lock. Profile operations, including installation, use
the existing Rust writer marker and check the same preconditions there. An
installation can change inventory without changing activation generation; the
digest detects that difference. Exact source bytes are checked again before an
installation effect. Legacy CLI writers use those same locks.

Each guarded manager captures its result and post-inventory before releasing the
mutation lock. The adapter records operation ID, request digest, store digest,
post-state and result digest as immutable evidence. Canonical before/after inventories
are retained as private configuration artifacts in a separate snapshot directory;
receipts contain only their digests. Package/configuration text,
notices, credentials and model content are absent from operation evidence.
The evidence file, package filesystem changes and audit SQLite are not one atomic
transaction. Missing result records or incomplete evidence stay uncertain;
reconciliation reads a complete receipt and never repeats the package operation.

`external_change` reports a difference from the last confirmed adapter inventory,
including an unconfirmed management change. It assigns no actor. A fresh snapshot
can admit a newly reviewed operation; a stale one rejects. A restarted adapter
has a fresh controller epoch, so earlier preflight state cannot be reused.

Native helpers have bounded output and execution deadlines and inherit no ambient
environment. An unconfirmed helper result is uncertain. Protect registered source
files, Python and the manager script against other host administrators. Unix
native locks release on process exit; an interrupted profile writer leaves its
existing recovery marker for explicit inspection. Do not automatically remove
markers, repair activation, erase evidence or replay uncertain operations.

## Verification and recovery

```sh
cargo test -p gateway-management-extensions --locked
cargo test -p agent-response-gateway --test profile_packs --locked
python3 -B -m unittest discover -s scripts/tests -p test_extension_manager.py -v
```

Native Rust fixtures resolve `python3` by default and require Python 3.11+.
Set `MANAGEMENT_TEST_PYTHON` to an explicit supported interpreter when needed.
This test setting does not configure a production driver.

Synthetic tests exercise exact installs, stale generations and inventory digests,
corruption, fresh grants, preserved older versions/state, external CLI changes,
audit admission/result failures and receipt reconciliation. Native fixtures use
inert package bytes, including codec v2; no package code or live provider is run.
Runtime/library hosts and final distributions require their separate acceptance
checks. Preserve compatible package stores, private adapter evidence and the
separate audit journal when reverting binaries. Selecting an old package does not
reverse data formats or replay completed model work.
