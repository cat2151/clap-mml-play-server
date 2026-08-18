@echo off
chcp 65001 > nul
rem cross_repo_local_on.bat で切り替えたローカル横断ビルドを解除して git 依存へ戻す。
rem ON の間は Cargo.lock の該当エントリがローカルパスへ書き換わるので、ここで必ず戻す。
setlocal
cd /d "%~dp0"

if exist ".cargo\config.toml" (
    del ".cargo\config.toml"
    rem .cargo が空になったときだけ消える（他の設定を置いていれば残る）。
    rmdir ".cargo" 2>nul
    echo .cargo\config.toml を削除しました。
) else (
    echo ローカル横断ビルドは OFF のままです。
)

git checkout -- Cargo.lock
if errorlevel 1 (
    echo [WARN] Cargo.lock を復元できませんでした。git status を確認してください。
) else (
    echo Cargo.lock を HEAD の内容へ戻しました。
)
endlocal
