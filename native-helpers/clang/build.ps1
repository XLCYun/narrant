# 仅构建本工具，不从被分析项目查找或执行构建逻辑。
param(
    [Parameter(Mandatory = $true)][string]$LlvmRoot,
    [string]$BuildDirectory = (Join-Path $PSScriptRoot "build")
)
$ErrorActionPreference = "Stop"
$LlvmRoot = (Resolve-Path $LlvmRoot).Path
& cmake -S $PSScriptRoot -B $BuildDirectory "-DLLVM_DIR=$LlvmRoot/lib/cmake/llvm" "-DClang_DIR=$LlvmRoot/lib/cmake/clang" -DCMAKE_BUILD_TYPE=Release
if ($LASTEXITCODE -ne 0) { throw "CMake configuration failed: $LASTEXITCODE" }
& cmake --build $BuildDirectory --config Release --parallel 2
if ($LASTEXITCODE -ne 0) { throw "Native helper build failed: $LASTEXITCODE" }
$Helper = Join-Path $BuildDirectory "narrant-clang.exe"
if (-not (Test-Path $Helper)) { throw "Native helper output missing: $Helper" }
Write-Output $Helper
