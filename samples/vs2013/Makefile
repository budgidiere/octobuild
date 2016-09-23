# See http://blog.airesoft.co.uk/2013/01/
C2=/B2 C:\Work\github\octobuild\target\debug\octobuild.dll

all: sample.cpp.o

clean:
	del *.h.pch
	del *.cpp.o

sample.h.pch: sample.h
	cl /c /Bd /Bt /Bk $(C2) /DFOO=1 /D BAR=2 /Ycsample.h /Fpsample.h.pch /Foprecompiled.cpp.o precompiled.cpp

sample.cpp.o: sample.cpp sample.h.pch
	cl /c /Bd /Bt /Bk $(C2) /DFOO=1 /D BAR=2 /Yusample.h /Fpsample.h.pch /Fosample.cpp.o sample.cpp
