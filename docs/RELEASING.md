# Releasing CreaBIM

Versions follow semantic versioning (`MAJOR.MINOR.PATCH`). Each release is also named by the day it was tagged, so the release and its files carry both:
- release: **CreaBIM 1.0.0 · 2026-11-09**
- files: `CreaBIM-1.0.0-2026-11-09-windows-x64.zip`, `CreaBIM-1.0.0-2026-11-09-linux-x64.tar.gz`, `CreaBIM-1.0.0-2026-11-09-linux-x64.deb`, `CreaBIM-1.0.0-2026-11-09-linux-x64.rpm`, `CreaBIM-1.0.0-2026-11-09-macos-universal.dmg`, `SHA256SUMS.txt`

The date is ISO (year-month-day), so it reads the same in every country and sorts in order.

Steps:
1. Set `version` in the workspace `Cargo.toml`. Run the checks:
   - `cargo fmt --all --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`

   Then merge to `main`.
2. Tag the merge commit and push the tag:
   - `git tag v1.0.0`
   - `git push origin v1.0.0`
3. The **Release** workflow builds Windows, Linux (including Ubuntu 22.04+ `.deb` and RHEL 10 / CentOS Stream 10-compatible `.rpm` packages), and a universal macOS `.dmg`, writes the checksums, and opens a **draft** release.
4. Read the draft, add highlights, then publish it.

Download links always point at `https://github.com/chopinregis/CreaBIM/releases/latest`.

## Building release packages locally

The examples below run from the repository root. Replace the sample version and date with the release values. The package scripts take `VERSION DATE TARGET_DIR OUTPUT_FILE`.

### Linux packages

Build on Ubuntu 22.04 x86-64 so the binaries use glibc 2.35 and remain compatible with Ubuntu 22.04, Linux Mint 21, newer Ubuntu releases, and EL10. Install the build dependencies and compile both binaries:

```sh
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev rpm
cargo build --release -p creabim-app -p creabim-cli
```

Create the Debian and RPM packages:

```sh
bash packaging/linux/make-deb.sh 1.1.1 2026-11-12 target/release \
  dist/CreaBIM-1.1.1-2026-11-12-linux-x64.deb
bash packaging/linux/make-rpm.sh 1.1.1 2026-11-12 target/release \
  dist/CreaBIM-1.1.1-2026-11-12-linux-x64.rpm
```

The DEB requires `libc6` 2.35 or later. The RPM requires glibc 2.35 or later, GTK 3, WebKitGTK 4.1 and `libxkbcommon-x11`. On CentOS Stream 10, enable CRB and EPEL 10; on RHEL 10, enable CodeReady Builder and EPEL 10, which provide the WebKitGTK 4.1 runtime dependency.

### macOS universal disk image

Build on macOS with Xcode command-line tools installed. The universal `.dmg` contains binaries for Apple Silicon and Intel Macs:

```sh
rustup target add x86_64-apple-darwin aarch64-apple-darwin
export MACOSX_DEPLOYMENT_TARGET=11.0
cargo build --release --target x86_64-apple-darwin -p creabim-app -p creabim-cli
cargo build --release --target aarch64-apple-darwin -p creabim-app -p creabim-cli
mkdir -p target/release-macos-universal
lipo -create target/x86_64-apple-darwin/release/creabim-app \
  target/aarch64-apple-darwin/release/creabim-app \
  -output target/release-macos-universal/creabim-app
lipo -create target/x86_64-apple-darwin/release/creabim \
  target/aarch64-apple-darwin/release/creabim \
  -output target/release-macos-universal/creabim
bash packaging/macos/make-dmg.sh 1.1.1 2026-11-12 target/release-macos-universal \
  dist/CreaBIM-1.1.1-2026-11-12-macos-universal.dmg
```
