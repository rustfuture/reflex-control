# Releasing

Pushing a tag such as `v0.4.0` runs `.github/workflows/release.yml`. It builds the
`reflex` CLI on native runners, packages it, and publishes a GitHub release that
`install.sh` and `install.ps1` download from.

## What a release contains

For version `X.Y.Z`, each target gets an archive and a checksum file:

| Target                       | Runner            | Archive                                          |
|------------------------------|-------------------|--------------------------------------------------|
| `x86_64-unknown-linux-gnu`   | `ubuntu-22.04`    | `reflex-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz`   |
| `aarch64-unknown-linux-gnu`  | `ubuntu-22.04-arm`| `reflex-X.Y.Z-aarch64-unknown-linux-gnu.tar.gz`  |
| `x86_64-apple-darwin`        | `macos-15-intel`  | `reflex-X.Y.Z-x86_64-apple-darwin.tar.gz`        |
| `aarch64-apple-darwin`       | `macos-latest`    | `reflex-X.Y.Z-aarch64-apple-darwin.tar.gz`       |
| `x86_64-pc-windows-msvc`     | `windows-latest`  | `reflex-X.Y.Z-x86_64-pc-windows-msvc.zip`        |

Each archive holds one folder, `reflex-X.Y.Z-<target>/`, with the binary, `LICENSE` and
`README.txt` (from `packaging/README.txt`). Each `<archive>.sha256` holds
`<sha256>  <archive name>`.

Why these runners:

- The Linux builds use the oldest hosted images so the binary links against glibc 2.35 and
  runs on most current distributions. They are glibc builds, not musl: Alpine is not
  supported by the installer, which points those users to `cargo install`.
- SQLite is bundled (`rusqlite` with `bundled`), so every runner compiles it with its own
  C compiler. All runners have one.
- Intel macOS builds on `macos-15-intel` because `macos-13` has been retired by GitHub.
  Change the runner label in `release.yml` if GitHub retires that one too.

## Cut a release

1. Pick the version. While the project is 0.x, use a minor bump for new features and a
   patch bump for fixes.
2. On a branch, set `version` under `[workspace.package]` in `Cargo.toml`, then refresh the
   lockfile and check the build:

   ```sh
   cargo build --release -p reflex-cli            # updates Cargo.lock; commit the change
   cargo build --release --locked -p reflex-cli   # must succeed, the workflow builds with --locked
   ./target/release/reflex --version              # must print the new version
   ```

3. In `CHANGELOG.md`, rename `## Unreleased` to `## X.Y.Z - YYYY-MM-DD` and add a fresh empty
   `## Unreleased` above it.
4. Open a pull request, wait for CI, and merge it into `main`.
5. Optional dry run: in GitHub, Actions, Release, "Run workflow" on `main`. It builds all five
   targets and attaches the archives to the run, but publishes nothing.
6. Tag the merge commit on `main` and push the tag:

   ```sh
   git checkout main && git pull
   git tag -a vX.Y.Z -m "reflex X.Y.Z"
   git push origin vX.Y.Z
   ```

   The workflow fails before building if the tag does not equal `v` plus the workspace
   version. If you tagged the wrong commit or version, delete the tag
   (`git push origin :refs/tags/vX.Y.Z`, `git tag -d vX.Y.Z`), fix it and tag again.
   Workflows run from the tagged commit, so `release.yml` must already be on that commit.

## Verify

1. Watch the Release workflow finish; the `Publish release` job only runs for tags.
2. Open the release page. It should list 10 files (5 archives, 5 `.sha256`) and the
   generated notes. Edit the notes to match the CHANGELOG entry if you like.
3. Check one download by hand:

   ```sh
   curl -fsSLO https://github.com/rustfuture/reflex-control/releases/download/vX.Y.Z/reflex-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz
   curl -fsSLO https://github.com/rustfuture/reflex-control/releases/download/vX.Y.Z/reflex-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz.sha256
   sha256sum -c reflex-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz.sha256   # shasum -a 256 -c on macOS
   ```

4. Run the installers on a machine without Rust, into a scratch directory:

   ```sh
   curl -fsSL https://raw.githubusercontent.com/rustfuture/reflex-control/main/install.sh \
     | REFLEX_VERSION=X.Y.Z REFLEX_INSTALL_DIR="$(mktemp -d)" sh
   ```

   On Windows: `$env:REFLEX_VERSION='X.Y.Z'; irm https://raw.githubusercontent.com/rustfuture/reflex-control/main/install.ps1 | iex`

   Both print `reflex X.Y.Z`. Without `REFLEX_VERSION` they install the latest release.
The installers only work for versions that have archives, so they cannot install releases tagged before this
workflow existed (v0.3.0 and earlier).
5. Run `reflex install` in a scratch git repository and `reflex doctor`.

Tags are the trigger, so a tag pushed for a release that later turns out to be broken
should be fixed with a new patch release. Deleting a published release and its tag is
possible but breaks anyone who already installed that version and checked its checksum.

## Testing the installers

Both installers accept `REFLEX_DOWNLOAD_BASE`, a base URL (or `file://` URL, or a local
directory for `install.ps1`) that holds the archive and its `.sha256`. To try a local build:

```sh
cargo build --release --locked -p reflex-cli --target "$(rustc -vV | sed -n 's/^host: //p')"
# package it the way the workflow does (see the "Package" steps in release.yml) into ./dist, then:
REFLEX_DOWNLOAD_BASE="file://$PWD/dist" REFLEX_VERSION=X.Y.Z REFLEX_INSTALL_DIR="$(mktemp -d)" sh install.sh
```

Run `shellcheck install.sh` after changing the script.
