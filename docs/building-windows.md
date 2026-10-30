# Building on Windows

A native build with Microsoft's toolchain (MSVC), edited and run from VS Code. The Windows release asset still comes
from `release/build-windows.sh` (a MinGW cross build on Linux); this is for working on the client on Windows.

## Install once

From a PowerShell window:

```powershell
winget install --id Git.Git
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id Rustlang.Rustup
winget install --id Microsoft.VisualStudioCode
```

- The Build Tools give the MSVC compiler and linker and the Windows SDK (rc.exe for the exe's icon). With Visual
  Studio 2022 Community already installed, add the "Desktop development with C++" workload instead.
- rustup installs the `stable-x86_64-pc-windows-msvc` toolchain by default. Open a new terminal afterwards so `cargo`
  is on the PATH.

No CMake, NASM or other tools are needed: the only C and C++ code is ring and UnRAR, built by MSVC through Cargo.

## Get the source

Clone the repository (`git clone git@github.com:lenonk/vana-client.git`, or copy the folder over). Commits made on
the Linux machine reach Windows only once they are pushed or copied.

## Build and run

1. Open the folder in VS Code and install the recommended extensions when it asks (rust-analyzer, C/C++, Even
   Better TOML).
2. **Ctrl+Shift+B** builds `target\release\vana-client.exe`. The first build takes several minutes; later ones are
   incremental.
3. **F5** builds and starts the client under the debugger. "vana-client (release with debug info)" is release speed
   with symbols (`target\release-debug`); pick it or "vana-client (release)" in the Run and Debug view.

Terminal > Run Task has the rest: Run (`cargo run --release`, the log in that terminal), Test, and the dist build
(`--profile dist`, full LTO, slow to link).

The exe runs without a console window; started from a terminal it writes its log there. The MSVC build links the C
runtime statically, so the exe runs on another PC without the Visual C++ redistributable.

## Where things go

| What | Where |
| --- | --- |
| Config | `%APPDATA%\vana-client\config.json` |
| Log (and the run before) | `%APPDATA%\vana-client\vana-client.log`, `vana-client.prev.log` |
| Game files (first start) | `%APPDATA%\vana-client\game` unless another folder is picked |
| Mods | `%APPDATA%\vana-client\mods` |

## When it fails

- `link.exe` not found, or `cl.exe` errors from ring or unrar_sys: the C++ workload is missing. Re-run the Build Tools
  installer and add "Desktop development with C++" (MSVC v143 and a Windows SDK).
- `warning: no exe icon`: rc.exe was not found (no Windows SDK). The build goes on without the icon.
- rust-analyzer shows errors that `cargo build` does not: reload the window after the first build.
