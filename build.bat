cd %~dp0
cargo build --release --target i686-pc-windows-gnu || exit 1
cargo build --release --target x86_64-pc-windows-gnu || exit 1
copy /Y target\i686-pc-windows-gnu\release\octobuild.dll target\octobuild.x86.dll
copy /Y target\x86_64-pc-windows-gnu\release\octobuild.dll target\octobuild.x64.dll
%WIXSHARP_DIR%\cscs.exe wixcs\setup.cs || exit 1
