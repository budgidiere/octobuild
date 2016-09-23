@call "%VS120COMNTOOLS%\..\..\VC\vcvarsall.bat" amd64
@set OCTOBUILD_CACHE=%~dp0cache
nmake clean all
