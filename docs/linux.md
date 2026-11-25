# Greign for Linux

The release workflow builds the x86-64 Debian package on Ubuntu 22.04. Packages built on newer distributions may require a newer glibc. The editor needs an OpenGL-capable graphical session. Both Auto Upright models run locally on the CPU.

1. Save `Greign-linux-amd64.deb` to your Downloads folder. Published packages appear under Assets on the [private releases page](https://github.com/gwcmowry/Greign/releases).
2. From the download directory, run `sudo apt install ./Greign-linux-amd64.deb`.
3. Launch **Greign** from the applications menu or run `greign`. Choose a folder of camera scans to import your first roll.

The installed app saves edits under `~/.local/share/greign/.greign/` and exports under `~/.local/share/greign/exports/film-rolls/` by default. It honors `XDG_DATA_HOME` and the `GREIGN_WORKSPACE` override. You can choose an export destination in the app. Install a newer package with the same apt command. Remove the app with `sudo apt remove greign`; your scans, edits and exports stay in your account.

For a source build on other Linux systems, see the [repository launch instructions](../README.md#native-film-workspace).
