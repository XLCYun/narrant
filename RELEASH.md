
# 1. 全量测试

请先跑一次当前系统下的全量测试。

# 2. 打新版本 release

当前仓库为 Private 仓库，实际的公共发布仓库在 https://github.com/cyunlab/narrant 。

当要发布一个新版本时，先检查当前仓库打的最的一个 tag 是 va.b.c：
1. 发大版本时，上升为 v{a+1}.b.c
2. 发中版本时，上升为 va.{b+1}.c
3. 发小版本时，上升为 va.b.{c+1}
确定后使用 `git tag` 打上新标签。

更新 package.json

# 3. 创建 release

查看从 va.b.c 到当前新版本的 git log 日志，总结版本变动内容。使用 `gh` 命令在 cyunlab/narrant 仓库创建一个新的 release，描述使用版本变动内容。

## 4. 发布 artifact

若当前在 macos 系统，使用命令 `pnpm run build:macos:signed` 构建新的带签名的 macos 安装包。

若当前在 windows 系统，使用命令 `pnpm run build:windows` 构建 windows 安装包。

将构建的安装包上传到 cyunlab/narrant 新创建的 release 中。
