all: sample.cpp.o obj/sample2.obj issue-12.cpp.o

clean:
	del *.h.pch
	del *.cpp.o
	del *.i

sample.h.pch: sample.h
	octo_cl /c /DTEST /Ycsample.h /Fpsample.h.pch /Foprecompiled.cpp.o precompiled.cpp
	
sample.cpp.o: sample.h.pch
	octo_cl /c /DTEST /Yusample.h /Fpsample.h.pch /Fosample.cpp.o sample.cpp
	
obj/sample2.obj: sample.h.pch sample2.cpp sample3.cpp
	octo_cl /c /DTEST /Yusample.h /Fpsample.h.pch /FIsample.h /Foobj/ sample2.cpp sample3.cpp
	
issue-12.cpp.o: issue-12.cpp
	octo_cl /c /EHsc /Foissue-12.cpp.o issue-12.cpp
