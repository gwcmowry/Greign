# Greign for macOS

On an Apple Silicon Mac (M1 or newer) running macOS 12 or later:

1. Sign in to a GitHub account with access to Greign and [download Greign-macos-arm64.dmg](https://github.com/gwcmowry/Greign/releases/download/v0.1.0-beta.1/Greign-macos-arm64.dmg).
2. Open the disk image and drag Greign into Applications.
3. Open Greign from Applications. If Gatekeeper blocks the first launch, open System Settings → Privacy & Security (System Preferences → Security & Privacy on Monterey), choose **Open Anyway** for Greign and confirm. The beta is ad-hoc signed and has not been notarized.

Both orientation models are bundled and run on the CPU. The DMG includes everything needed to run the editor. A ZIP of the same app is available on the [release page](https://github.com/gwcmowry/Greign/releases/tag/v0.1.0-beta.1).

Import your scans with the roll import control. The downloaded app opens with
an empty library; private demo photographs are not distributed.
Command-Z undoes edits; Command-C/V copies/pastes frame adjustments;
Command-E opens export. Command-scroll zooms around the pointer.

Your library and edits are saved under
`~/Library/Application Support/Greign/.greign/rolls.json`; diagnostic timings
are stored alongside it. Default exports go to `~/Pictures/Greign`.
Original scans are referenced in place and must remain available.
Installing a newer app preserves these files.

The editor uses native OpenGL preview with CPU fallback. The separate compute
CLI uses Metal on macOS. Actual film color accuracy and Lightroom DNG handoff
still need the validation described in the main README.

## Build from source

For a first build, install the Xcode Command Line Tools with `xcode-select --install`
and let the installer finish. If you use Homebrew, install Rust via rustup and
install CMake with `brew install rustup cmake`. If this is your first rustup
installation, run `rustup default stable`. The checkout pins the compiler it uses.

Clone the private repository using a GitHub account with access, then build and
launch the editor from Terminal.

```sh
git clone https://github.com/gwcmowry/Greign.git
cd Greign
cargo run --release --locked -p greign-app
```

For an existing checkout, run `git switch main` and `git pull --ff-only` in
the repository, then repeat the Cargo command above.

To make a Finder-launchable app and disk image from the same checkout, run

```sh
./scripts/build-macos.sh
```

The build script produces a ZIP and DMG under `dist/`. GitHub Actions builds
Apple Silicon, runs workspace tests, checks the bundle signature and
linked libraries, and exercises native preview using generated test images.
The test images are synthetic; they do not validate real film color.

For a workspace with data stored separately, set `GREIGN_WORKSPACE` to a
writable folder. This also keeps exports inside that workspace.
`GREIGN_LIBRARY_PATH` can override the library file; `GREIGN_CPU_PREVIEW=1`
selects CPU preview for troubleshooting.

The independent darktable modification patch is included in the source
repository, but is not part of this app.
