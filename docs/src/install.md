# Installing

You don't have to install anything to use System4Dispatch: the web app runs at
<https://www.uncommonmodels.com/System4Dispatch/>. Install the **desktop app**
if you want to connect over the network (GridConnect TCP to JMRI, an Ethernet
LCC buffer or `rolcb-hub`), or prefer a normal application window.

## Downloads

Every [release](https://github.com/UncommonModels/System4Dispatch/releases) has
these files (`vX.Y.Z` is the version):

| File | For |
|---|---|
| `System4Dispatch-vX.Y.Z-linux-x86_64.deb` | Debian, Ubuntu, Linux Mint (64-bit PCs) |
| `System4Dispatch-vX.Y.Z-linux-x86_64.rpm` | Fedora, openSUSE, RHEL |
| `System4Dispatch-vX.Y.Z-linux-x86_64.AppImage` | Any Linux distro, no install needed |
| `System4Dispatch-vX.Y.Z-linux-x86_64.flatpak` | Any Linux distro with Flatpak |
| `System4Dispatch-vX.Y.Z-linux-arm64.deb` / `.rpm` | Raspberry Pi 4/5 with a 64-bit OS, other ARM64 Linux |
| `System4Dispatch-vX.Y.Z-macos-arm64.dmg` | Macs with Apple silicon (M1 and later) |
| `System4Dispatch-vX.Y.Z-macos-x86_64.dmg` | Intel Macs |
| `System4Dispatch-vX.Y.Z-windows-x86_64.msi` / `-setup.exe` | Windows 10 and 11 |
| `System4Dispatch-vX.Y.Z-web.zip` | The web app, to host on your own web server |
| `SHA256SUMS` | Checksums of all of the above |

## Linux

- **deb:** `sudo apt install ./System4Dispatch-*.deb`
- **rpm:** `sudo dnf install ./System4Dispatch-*.rpm`
- **AppImage:** `chmod +x System4Dispatch-*.AppImage` and run it. On distros
  without FUSE 2, run it with `--appimage-extract-and-run`.
- **Flatpak:** `flatpak install --user System4Dispatch-*.flatpak`. It needs the
  Flathub remote, which provides the GNOME runtime.

To use a USB adapter, your user needs access to the serial port. On most
distros that means joining the `dialout` group (`uucp` on Arch):
`sudo usermod -aG dialout $USER`, then log out and back in.

## macOS

Open the `.dmg` and drag System4Dispatch to Applications. The app isn't
notarised yet, so the first time macOS will refuse to open it: right-click the
app, choose **Open**, then confirm. Or run
`xattr -d com.apple.quarantine /Applications/System4Dispatch.app`.

## Windows

Run the `.msi` (or the `-setup.exe`). Windows SmartScreen may warn about an
unrecognised app; choose **More info → Run anyway**. The app uses Microsoft's WebView2
runtime, which Windows 11 and up-to-date Windows 10 already have; if it's missing,
the installer offers to download it.

## Self-hosting the web app

Unzip `System4Dispatch-vX.Y.Z-web.zip` and serve the folder from the root of a
web server. USB serial in the browser only works over **https** (or on
`localhost`), because Web Serial requires a secure page.

## Nightly builds

Untested snapshots of the latest code are published as the
[`nightly` pre-release](https://github.com/UncommonModels/System4Dispatch/releases/tag/nightly),
with an AppImage and a Flatpak. Use a numbered release for operating sessions.
