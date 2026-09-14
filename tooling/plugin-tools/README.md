# Native plugin tools

The source-inclusive tool archive provides the Python standard-library package
manager independently of a gateway checkout. Python 3.11+ on Linux or macOS is
required for host installation. No Python packages are downloaded. Supplied
plugin bytes are never executed by packaging, inspection, or installation.

## Produce a distribution

The default base is `python:3.14.0-slim-bookworm` pinned to the multi-platform
manifest `sha256:d13fa0424035d290decef3d575cea23d1b7d5952cdf429df8f5542c71e961576`.
For an explicit update, select and review an official Python 3.11–3.14 exact patch `slim-bookworm` image
and obtain its immutable manifest digest from the registry. Supply the complete
`python:<patch>-slim-bookworm@sha256:<digest>` as `PLUGIN_TOOLS_BASE`. The optional `--base-image "$PLUGIN_TOOLS_BASE"` override rejects a floating tag. This pins the entire runtime; no package installation is
performed by the generated Dockerfile. The archive does not establish approval
of the base image or third-party redistribution rights.

```sh
mkdir -p .local
python3.14 -B scripts/build_plugin_tools.py \
  --output .local/plugin-tools.tar
```

The output SHA-256 and `SHA256SUMS` identify the distributed source. Retain the
exact source archive with an image distribution; also provide the selected base
image's dependency notices and corresponding source obligations under the
release policy. The assembly helper is included as source evidence; assembling
another archive uses the source checkout layout.

Extract into a new empty directory and build that directory as the entire build
context. Preload the approved base image before offline building:

```sh
docker build --network=none --pull=false -t plugin-tools:local .
```

The generated Dockerfile copies only explicitly included files. A standalone
standard-library conformance script can be added with `--conformance PATH`;
without it no conformance implementation is claimed. Language compilers belong
in separately selected build environments; this image packages already-built
artifacts and does not compile arbitrary plugin source.

## Static inspection across platforms

`package --target macos-arm64` and `inspect --target macos-arm64` select an
artifact platform independently of the Linux tool image. Omitting `--target`
preserves host matching. Target declarations do not prove ABI compatibility or
successful execution. `install` and `enable` have no target override and retain
host, ownership, permission, digest and link checks.

Mount only the input package read-only for static inspection. `$PACKAGE_DIR`
must be an absolute directory and `$PACKAGE_SHA256` a trusted manifest digest:

```sh
docker run --rm --network=none --read-only --cap-drop=ALL \
  --security-opt=no-new-privileges --pids-limit=64 --memory=256m --cpus=1 \
  --user "$(id -u):$(id -g)" \
  --mount "type=bind,src=$PACKAGE_DIR,dst=/package,readonly" \
  plugin-tools:local inspect --package /package \
  --expected-sha256 "$PACKAGE_SHA256" --target macos-arm64
```

For packaging, mount only a dedicated artifact input directory read-only and a
dedicated empty output parent writable. Pass absolute container paths to
`package --binary`, `--license-file`, and `--output`; the output must not exist.
Do not mount the checkout, operational extension store, credentials, home, or
Docker socket. Do not pass operational environment variables.

For optional Linux conformance, override the entrypoint with
`--entrypoint python3` and pass `-I -B /opt/plugin-tools/plugin_conformance.py`
plus its documented arguments. Use only synthetic fixtures, a dedicated writable
scratch mount and the same offline/resource restrictions. Executable fixtures
must target the container's Linux architecture. Bind ownership must allow the
selected numeric user access. A macOS host must perform separate native runtime
validation; Linux container execution does not validate macOS binaries.

## Host installation and limits

Run the extracted `extension_manager.py` directly on the actual runtime host for
final inspection, installation and activation. A Linux container can install
into a container-owned Linux store, but installing into a macOS runtime store
through bind mounts is unsupported. Container UID and path mappings do not
replace the host's final checks.

The tooling container is a development and verification environment, not the
product plugin sandbox. Native runtime plugins remain explicitly trusted code.
Static inspection neither executes plugins nor certifies their behavior. No
image publication, automatic updates, live provider calls or runtime downloads
are performed by these tools.
