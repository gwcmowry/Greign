# Build Greign on Windows

Use a 64-bit Windows PC with the MSVC toolchain. Install Git, Rust via [rustup](https://rustup.rs/), and Visual Studio 2022 Build Tools with **Desktop development with C++**, the Windows SDK and CMake. Install NASM and add its installation directory to PATH for the JPEG codec build.

Open **Developer PowerShell for VS 2022** and run:

```powershell
git clone https://github.com/gwcmowry/Greign.git
cd Greign
cargo build --release --locked -p greign-app
.\target\release\greign-app.exe
```

The private beta requires a GitHub account with repository access. The checkout pins the Rust version. Auto Upright uses the CPU and includes its models in the executable. The source-tree build stores the library under `.greign` in the checkout; a relocated executable uses `%LOCALAPPDATA%\Greign`. You can set `GREIGN_WORKSPACE` to choose a different writable folder.
