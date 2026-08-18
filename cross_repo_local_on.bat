@echo off
chcp 65001 > nul
rem 兄弟 repo (..\clap-mml-render-tui) をローカルパスで直接参照するビルドへ切り替える。
rem cross-repo の変更を「両 repo を同時に書いて、push する前にローカルで検証」するためのもの。
rem 生成する .cargo\config.toml は .gitignore 済み。絶対パスは書かない（相対パスのみ）。
setlocal
cd /d "%~dp0"

set "SIBLING=..\clap-mml-render-tui"
if not exist "%SIBLING%\cmrt-runtime\Cargo.toml" (
    echo [ERROR] 兄弟 repo が見つかりません: %SIBLING%
    echo         この repo と clap-mml-render-tui を同じ親ディレクトリへ並べて置いてください。
    exit /b 1
)

if not exist ".cargo" mkdir ".cargo"
> ".cargo\config.toml" echo # cross_repo_local_on.bat が生成。commit しないこと（.gitignore 済み）。
>> ".cargo\config.toml" echo [patch."https://github.com/cat2151/clap-mml-render-tui"]
>> ".cargo\config.toml" echo cmrt-runtime = { path = "../clap-mml-render-tui/cmrt-runtime" }

echo ローカル横断ビルドを ON にしました。
echo   clap-mml-render-tui の cmrt-runtime を %SIBLING%\cmrt-runtime から解決します。
echo 戻すときは cross_repo_local_off.bat を実行してください（Cargo.lock も戻します）。
endlocal
