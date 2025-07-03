npm run build
copy /Y src\assets\DialogFlowAiSDK.js ..\sdk\javascript\.
del /S /Q ..\src\resources\assets\*
xcopy /S dist\* ..\src\resources\assets\.