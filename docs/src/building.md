# Building and releasing

## Prerequisites

- **Rust** (stable), via [rustup](https://rustup.rs).
- The **WebAssembly target**, for the web build:

  ```sh
  rustup target add wasm32-unknown-unknown
  ```

- The **Dioxus CLI**, version 0.7.10 to match the `dioxus` dependency:

  ```sh
  cargo install dioxus-cli --version 0.7.10 --locked
  ```

- **rolcb** checked out **next to** this repository, because it is a path dependency (`../rolcb`):

  ```sh
  git clone https://github.com/UncommonModels/rolcb.git
  git clone https://github.com/UncommonModels/System4Dispatch.git
  ```

- For the **desktop build on Linux**, the WebKitGTK and GTK development packages:

  ```sh
  # Fedora
  sudo dnf install webkit2gtk4.1-devel gtk3-devel libxdo-devel libsoup3-devel systemd-devel
  # Debian / Ubuntu
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libudev-dev
  ```

  macOS and Windows need no extra packages beyond their usual build tools (Xcode command-line tools; Visual Studio Build Tools).

## Running during development

Run these from the `System4Dispatch` directory:

```sh
cargo test                                         # core logic tests (host)
dx serve -p system4dispatch                        # web app at http://localhost:8080
dx serve -p system4dispatch --platform desktop     # desktop app
```

`dx serve` rebuilds and reloads when files change; `Dioxus.toml` also watches `crates/core/src`. Web Serial works on `http://localhost`, so you can test a USB adapter with the dev server in Chrome or Edge.

The workspace's default member is `crates/core`, so plain `cargo build` and `cargo test` don't try to build the WASM app. To type-check the app without `dx`, use the same commands as CI:

```sh
cargo clippy -p system4dispatch --target wasm32-unknown-unknown -- -D warnings          # web
cargo clippy -p system4dispatch --no-default-features --features desktop -- -D warnings  # desktop
```

## Release builds

```sh
dx bundle -p system4dispatch --platform web --release        # static site
dx bundle -p system4dispatch --platform desktop --release    # installers for this OS
```

The web bundle lands in `target/dx/system4dispatch/release/web/public/`: static files that can be served from any web server. When the site lives under a sub-path (as on GitHub Pages), pass `--base-path`, for example `--base-path System4Dispatch`. Serve it over `https` (or from `localhost`), since Web Serial and the offline service worker both require a secure context.

The desktop bundle produces the platform's native packages: `.deb`, `.rpm` and `.AppImage` on Linux, `.dmg` on macOS, and `.msi` or a setup `.exe` on Windows. Use `--out-dir dist` to collect them in one place. Bundle metadata (identifier `com.uncommonmodels.system4dispatch`, publisher, description, category) is in `crates/app/Dioxus.toml`.

The release profile optimises for size (`opt-level = "z"`, LTO, one codegen unit), which matters most for the WASM download.

## Documentation

This book lives in `docs/` and is built with [mdBook](https://rust-lang.github.io/mdBook/):

```sh
cargo install mdbook --locked
mdbook serve docs --open      # live preview
mdbook build docs             # output in docs/book/
```

It is published at <https://www.uncommonmodels.com/System4Dispatch/docs/>, next to the web app.

## Continuous integration

GitHub Actions workflows live in `.github/workflows/`. Each one checks out `System4Dispatch` and `rolcb` side by side, because of the path dependency.

| Workflow | Runs on | What it does |
|---|---|---|
| **CI** (`ci.yml`) | pushes to `main`, pull requests | Installs the Linux desktop dependencies, runs `cargo test`, and runs Clippy with `-D warnings` for both the web (`wasm32-unknown-unknown`) and desktop builds. |
| **Pages** (`pages.yml`) | pushes to `main`, manual | Builds the web app with `dx bundle --platform web --release --base-path <repo name>` and deploys it to GitHub Pages, served at <https://www.uncommonmodels.com/System4Dispatch/>. |
| **Release** (`release.yml`) | `v*` tags, manual | Builds every download listed in [Installing](install.md): desktop installers for Linux x86_64 (deb, rpm, AppImage), Linux arm64 (deb, rpm), macOS arm64 and x86_64 (dmg) and Windows (msi, setup.exe), a Flatpak, and a zip of the web app. On a tag it attaches them with `SHA256SUMS` to the GitHub release; a manual run only uploads workflow artifacts (a dry run). |
| **Nightly** | nightly | Nightly builds are published as a GitHub pre-release tagged `nightly`, with an AppImage and a Flatpak bundle. |

### Making a release

1. Update `version` under `[workspace.package]` in the root `Cargo.toml` (it is shown in the app's Settings and Help, and reported to the bus as the node's software version).
2. Make sure CI is green on `main`.
3. Tag and push:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. The Release workflow builds the installers and attaches them to the GitHub release for that tag. The web app is deployed separately by the Pages workflow on every push to `main`.
