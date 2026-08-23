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
