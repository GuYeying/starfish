@echo off
rem 在 VS 开发者环境中运行 cargo（MSVC link.exe / INCLUDE / LIB 就位）。
rem 用法：scripts\vs-cargo.bat <cargo 参数…>
rem 例：  scripts\vs-cargo.bat check --workspace --all-targets --target wasm32-unknown-unknown
call "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=amd64 -no_logo
cargo %*
