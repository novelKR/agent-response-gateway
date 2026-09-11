# Native extension host: architecture and security contract

[English](README.md) | [한국어](README.ko.md)

This document distinguishes the extension foundation implemented by this change from the later Codex account-pool work tracked in issue #54. A native extension is separately installed trusted executable code. It is not a Rust dynamic library, an agent worker, or an operating-system sandbox.

## Goals and implemented boundary

Keep the default gateway small and unchanged when no extension activation lock is supplied. Provide explicit offline installation, approval of requested permissions, immutable package selection, restart-based activation, and supervised execution of an optional metadata observer.

The initial executable role is `observer`. The gateway supplies bounded numeric HTTP outcome metadata, not prompts, request or response bodies, authentication headers, account credentials, or tool results. The example executable and synthetic integration probe demonstrate this role. They do not implement ChatGPT login, quota polling, account rotation, or Codex Pool.

## Architecture

```text
Consumer / agent
    |
    | Responses HTTP
    v
Gateway core
    |-- local authentication and request admission
    |-- route and capability checks
    |-- provider transport and SSE backpressure
    |-- cancellation, capacity and retry policy
    |
    +-- bounded observation queue
            |
            v
       Native extension supervisor
            |
            | versioned local control messages
            v
       Trusted observer executable
```

Model traffic remains on the gateway's existing transport path. An observer is not an additional model proxy and does not receive every SSE fragment. Queue saturation or an observer failure must not cause an inference request to be repeated.

The host owns extension subprocess lifetime. The consuming application still owns tool execution, approvals, conversation history and recovery. Adding extension state does not add server-side Responses conversation storage.

## Package and activation lifecycle

Installation and activation are separate actions. Package preparation copies executable build artifacts into independent package files. Installation verifies the declared static manifest and packaged file inventory without executing the extension, including for metadata discovery. Reject unexpected files, invalid identifiers, unsafe paths, symbolic links, and unsupported package declarations. Installed package integrity checks must not be weakened merely because a compiler produced hard-linked build artifacts.

The package identity includes its version and content digest. Requested permissions do not grant themselves: the operator must approve the supported grants explicitly. Activation records exact package identities and grants in an atomically replaced lock. Do not discover or execute arbitrary programs from the shell PATH or an unreviewed directory.

The gateway loads an activation lock only when explicitly configured with `--extensions-lock`. Offline configuration and manifest inspection validate the activation and packages but must not execute an extension or contact an upstream service. Starting the gateway performs the runtime handshake with the selected executables. Changes take effect through a controlled stop/start, not hot replacement inside a running request.

A package checksum establishes integrity relative to the expected checksum; it does not independently establish a trustworthy publisher. The initial local package mechanism is not a public marketplace, remote updater, or publisher-signature verification service.

## Native trust model

Separate processes isolate address spaces and make child failures manageable, but they do not by themselves prevent a native executable from reading the user's files or accessing the network. Environment clearing and a narrow message protocol reduce accidental disclosure; they are not a sandbox for hostile native code.

Run only extensions the operator trusts. Logical role/grant checks constrain gateway-provided interfaces, not arbitrary operating-system calls made by a native executable. Untrusted third-party execution requires a separately reviewed OS isolation or WebAssembly design.

Avoid shell execution and inherited credentials. Keep private state and runtime ownership under the explicitly selected store. Protect files and directories, validate their type and identity, and reject concurrent runtime ownership instead of silently sharing mutable state. Do not log extension-provided strings as trusted diagnostics or expose unsanitized child output.

The initial native runtime contract is supported on Linux and macOS. Windows retains the extension-free gateway path; native extension execution must report an explicit unsupported condition rather than claiming equivalent ACL, locking or IPC behavior.

## Protocol, resource bounds and failure handling

Use a versioned, bounded handshake and observation/acknowledgement exchange. Reject unknown messages and malformed or oversized frames. Deadlines apply to complete frames, including a peer that sends only part of a message. Bounded queues protect the HTTP request path from an unresponsive observer. An observation is not a callback that can ask the gateway to execute tools, change routes or obtain secrets.

Keep child cleanup and shutdown bounded. Release runtime ownership and reap owned children on failure and shutdown. Do not automatically restart an inference request, replay observations as model work, or transfer a partial response to another account after an extension failure.

The core remains authoritative for request authentication, destinations, supported semantics, request and response limits, cancellation and transport retry policy. Installing a module must not enable redirects, inherited proxies, retries, fallback, remote access or silent request rewriting.

## Execution identity and compatibility

The optional extension configuration contributes exact package identities, grants and configuration to a separate extended execution manifest. Existing consumers that do not enable extensions retain the previous manifest/readiness contract. Consumers using the extended contract must validate the supported schema and the selected executable/configuration identities before starting work.

Immutable package/configuration identity is distinct from mutable operational state. Future token revisions and quota readings must not be disguised as package upgrades. Changes to extension code, permissions or policy must be considered during run binding and recovery, not applied behind the host's back.

## Codex Pool: planned extension, not implemented here

Codex Pool requires a dedicated, reviewed contract beyond the observer role. Do not send credentials or inference bodies through the observer protocol as an expedient implementation.

The intended responsibility split is:

| Component | Responsibility |
| --- | --- |
| Gateway core | Final admission, approved destination and credential use, bounded model transport, cancellation and attempt accounting |
| Codex Pool module | Authorized account lifecycle, quota/health evidence, account-selection proposals and account-bound state |
| Credential owner | One authoritative refresh lifecycle per grant; generation-safe updates and removal |
| Consumer host | Conversation origin binding, approvals, tool execution and explicit recovery or context migration |

A future pool must resolve a request-owned account and credential lease rather than mutate a global active token. The core must check the proposed account against the route, model, workspace and existing session binding. Account selection and changing an established conversation's authentication origin are different operations.

Encrypted reasoning and provider-local state must not be assumed portable across accounts. A missing or expired session binding after restart must not silently select another account for opaque history. The host must approve the appropriate continuity transition or start a fresh thread with verified portable context.

A rejected request, a request not yet dispatched, and a sent request with unknown acceptance require different retry decisions. No downstream bytes yet is not proof that the upstream did no work. Keep existing retry defaults; specify explicit budgets and safe evidence before adding any pool failover. Partial SSE, exposed tool output, cancellation and uncertain execution must not trigger transparent replay.

The pool's account/refresh state needs separate schema migration and rollback rules. Never run old and new owners concurrently against the same refresh grant. Do not automatically import a user's normal Codex login or restore stale credentials during a binary rollback.

## Evolution sequence

1. Keep internal role interfaces narrow and preserve the no-extension baseline.
2. Validate installation, activation, subprocess supervision and the numeric observer role with synthetic fixtures.
3. Review credential ownership, Codex transport, quota, session-binding and retry contracts before implementing Codex Pool.
4. Add independently useful roles only when their access and failure semantics are specified. A future protocol adapter requires a separate streaming/cancellation contract.
5. Consider a public SDK, signed distribution and WASM only after real extension implementations demonstrate the necessary stable boundaries.

Internal Rust modules and user-installable packages are different units. A Cargo feature can control a build, but is not by itself an installer. Multiple internal modules may ship as one coherent Codex Pool package rather than forcing users to assemble authentication, quota and scheduling components independently.

## Validation and release boundaries

Test offline installation without execution, exact grants, tampered packages, unsafe filesystem objects, concurrent ownership, malformed/partial handshakes, queue saturation, child death, cancellation and bounded shutdown. Assert that observer messages contain only the supported numeric metadata and that no-extension behavior remains unchanged.

The optional `metadata_observer` example and `scripts/extension_smoke.py` use synthetic data. Use the scripts' `--help` output for their current arguments. Run the repository's required formatting, Clippy, locked Rust tests, Python tests, publication/license checks and documentation checks for the exact PR head. Mock and hosted CI success do not establish live Codex-account qualification or consumer production acceptance.

The project license and third-party notice requirements continue to apply. Native-process separation is an engineering boundary, not a substitute for reviewing the rights and distribution terms of an extension or SDK.
