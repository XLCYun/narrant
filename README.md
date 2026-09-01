# narrant

## 构建 Windows 安装包

安装项目依赖后，在 Windows 系统的项目根目录运行：

```powershell
pnpm build:windows
```

该命令仅构建 NSIS `.exe` 安装包。构建产物位于：

```text
src-tauri\target\release\bundle\nsis\Narrant_1.0.0_x64-setup.exe
```

## 本地开发

Custom Babysit 的确定性 smoke、Venator 开发观测和数据保密边界见
[开发观测：Venator](docs/agents/dev-observability.md) 与 [Babysit smoke test](docs/agents/babysit-smoke.md)。
