@echo off
rem Compila step2wrl.exe in target\release\tools\step2wrl e ci copia le DLL che servono:
rem OpenCASCADE e librerie di terze parti (copy_dlls.py) e il runtime di Visual C++.
rem OCCT = cartella del pacchetto binario di OpenCASCADE (opencascade-X.Y.Z-vc14-64), vedi README.md
setlocal
if "%OCCT%"=="" set "OCCT=%USERPROFILE%\occt-8.0.1\opencascade-8.0.1-vc14-64"
set "HERE=%~dp0"
set "OUT=%HERE%..\..\target\release\tools\step2wrl"
set "PATH=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer;%PATH%"
for /f "usebackq delims=" %%i in (`vswhere.exe -latest -property installationPath`) do set "VS=%%i"
call "%VS%\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
if not exist "%OUT%" mkdir "%OUT%"
if not exist "%HERE%obj" mkdir "%HERE%obj"
cl /nologo /std:c++17 /O2 /EHsc /MD /W3 /utf-8 /DNDEBUG /D_USE_MATH_DEFINES /I"%OCCT%\inc" ^
   /Fo"%HERE%obj\\" "%HERE%main.cpp" /Fe"%OUT%\step2wrl.exe" ^
   /link /LIBPATH:"%OCCT%\win64\vc14\lib" ^
   TKernel.lib TKMath.lib TKBRep.lib TKG3d.lib TKMesh.lib TKLCAF.lib TKCAF.lib TKXCAF.lib TKDESTEP.lib TKXSBase.lib TKCDF.lib TKTopAlgo.lib TKG2d.lib || exit /b 1
del "%OUT%\step2wrl.exp" "%OUT%\step2wrl.lib" 2>nul
python -I "%HERE%copy_dlls.py" "%OCCT%" "%OUT%" || exit /b 1
rem Runtime di Visual C++ accanto all'eseguibile: funziona anche dove non è installato
copy /y "%VCToolsRedistDir%x64\Microsoft.VC143.CRT\*.dll" "%OUT%" >nul || exit /b 1
echo Pronto: %OUT%\step2wrl.exe
endlocal
