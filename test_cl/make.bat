@set PATH=%~dp0..\target\debug;%PATH%
@set OCTOBUILD_CACHE=%~dp0cache
@set RUST_BACKTRACE=1
cargo build --bin octo_cl || exit 1
@call "%VS120COMNTOOLS%\vsvars32.bat"
mkdir obj
nmake clean all && echo "OK"
