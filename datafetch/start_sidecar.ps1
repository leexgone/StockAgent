# 子木行情 sidecar 启动脚本（datafetch，ADR-0005：Python sidecar 127.0.0.1:8300）
# 用法（任选其一）：
#   1. 终端里：  cd E:\Github\StockAgent\datafetch; .\start_sidecar.ps1
#   2. 右键「使用 PowerShell 运行」（窗口标题会变成 zimu-datafetch sidecar）
#   3. 任何目录：powershell -ExecutionPolicy Bypass -File E:\Github\StockAgent\datafetch\start_sidecar.ps1
# 策略：优先用本目录 .venv（uv sync 建好的虚拟环境）；没有 .venv 时回退 uv run；
# 两者都缺则给出安装提示。停止服务 = 在本窗口 Ctrl+C 或直接关窗口。

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
try { $Host.UI.RawUI.WindowTitle = "zimu-datafetch sidecar (127.0.0.1:8300)" } catch { }

# 端口被占说明 sidecar 可能已在跑，先探一下，避免 uvicorn 报一串 Address in use
try {
    $alive = Invoke-RestMethod "http://127.0.0.1:8300/healthz" -TimeoutSec 2
    Write-Host "[start_sidecar] 8300 端口已有 sidecar 在跑（版本 $($alive.version)），无需重复启动。" -ForegroundColor Yellow
    exit 0
} catch { }

$venvPython = Join-Path $PSScriptRoot ".venv\Scripts\python.exe"
if (Test-Path $venvPython) {
    Write-Host "[start_sidecar] 使用 .venv 启动 uvicorn（Ctrl+C 停止）..." -ForegroundColor Cyan
    & $venvPython -m uvicorn zimu_datafetch.server:app --host 127.0.0.1 --port 8300
} elseif (Get-Command uv -ErrorAction SilentlyContinue) {
    Write-Host "[start_sidecar] 未找到 .venv，回退 uv run（首次请先在本目录执行 uv sync）..." -ForegroundColor Cyan
    uv run uvicorn zimu_datafetch.server:app --host 127.0.0.1 --port 8300
} else {
    Write-Host "[start_sidecar] 启动失败：既没有 .venv 也找不到 uv。" -ForegroundColor Red
    Write-Host "  请先安装 uv：winget install --id astral-sh.uv，重开终端后在本目录执行 uv sync；"
    Write-Host "  或者用其它 Python 环境安装依赖：pip install -e ."
    Read-Host "按回车退出"
    exit 1
}

$code = $LASTEXITCODE
Write-Host ""
if ($code -ne 0) {
    Write-Host "[start_sidecar] 进程异常退出（退出码 $code）。" -ForegroundColor Red
    Read-Host "按回车退出"
} else {
    Write-Host "[start_sidecar] 进程已退出（退出码 0）。"
}
exit $code
