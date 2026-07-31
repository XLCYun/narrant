# Frontend Product Experience

## Problem Statement

Narrant 已经能够围绕本地 **Repository Entry** 创建和持久化 **Review**，支持
**Pull Request Selection** 与 **Branch Selection**、显式刷新
**Review Snapshot**、浏览 **File Change**，以及通过 GitHub App 发现 Pull
Request。然而，现有前端把仓库注册、GitHub 授权、Review 历史、分支选择、更新检查和
Diff 阅读纵向堆叠在一个粗糙页面中，缺少稳定的信息架构、明确的状态反馈和适合长时间
代码阅读的空间分配。

用户打开应用后难以快速判断应该继续哪个 Review、如何开始新的 Review，以及当前操作
属于仓库管理还是代码阅读。已经存在的离线重开、Workspace Trust、局部 GitHub 失败、
特殊文件内容和安全 Markdown 渲染等能力，也没有形成一致、可发现的产品体验。

Narrant 还将面向中文和英文用户。当前界面文案直接写在组件中，中英文术语混杂；如果在
整体 UI 重构后才引入国际化，将重复搬迁新界面的全部文案。

## Solution

Narrant 将采用面向桌面代码审核的应用框架：左侧常驻、可调整和可折叠的主侧边栏用于
最近 Review、Repository Entry 和 Review 历史导航；右侧主区域根据当前选择展示首页、
仓库概览或 Review 阅读器。首页优先帮助用户继续最近 Review 或通过系统文件夹选择器
打开本地 Git clone。仓库概览通过 Pull Requests、本地分支和 Review 历史三个标签组织
创建与继续 Review 的入口。Review 阅读器把空间优先分配给文件树和 Monaco Diff，
Pull Request 描述与 revision 详情放入按需打开的右侧概览面板。

前端将优先复用现有 shadcn `base-maia` preset、taupe 基础色、Remix Icon、主题 token
和默认交互组件。视觉上采用克制的桌面开发工具风格，品牌绿色只承担主操作、选中和焦点
语义。首页和空状态保持舒展，仓库概览使用中等密度，主侧边栏和 Review 阅读器使用紧凑
密度。

本次重构同时引入 `i18next` 与 `react-i18next`。第一阶段支持简体中文和英文，默认跟随
系统语言，不提供手动语言选择器。翻译资源随应用离线打包，UI 偏好保存在前端本地存储，
不改变 Review 领域模型或 SQLite 持久化模型。

## User Stories

1. 作为首次使用 Narrant 的审核者，我希望看到清晰的品牌说明和“打开仓库文件夹”主操作，以便立即理解产品用途。
2. 作为首次使用 Narrant 的审核者，我希望知道代码内容来自本地仓库且产品执行只读审核，以便理解产品的安全边界。
3. 作为回访用户，我希望启动后看到最近 5 条 Review，并可展开到最近 10 条，以便快速继续上次工作且避免侧边栏过长。
4. 作为回访用户，我希望最近 Review 同时显示仓库、Review 身份和最后打开时间，以便辨认目标。
5. 作为回访用户，我希望点击最近 Review 的整行即可继续，以便减少导航步骤。
6. 作为维护多个仓库的审核者，我希望在常驻侧边栏中看到所有 Repository Entry，以便随时切换上下文。
7. 作为维护多个仓库的审核者，我希望展开一个仓库时先看到最近 10 条 Review，以便侧边栏保持紧凑。
8. 作为拥有大量 Review 历史的审核者，我希望点击“查看全部”后在侧边栏中看到该仓库的全部 Review，以便查找较早记录。
9. 作为拥有大量 Review 历史的审核者，我希望把完整列表收起到最近 10 条，以便恢复侧边栏空间。
10. 作为代码阅读者，我希望调整并折叠主侧边栏，以便把更多窗口空间留给 Diff。
11. 作为回访用户，我希望应用记住主侧边栏的宽度和折叠状态，以便下次保持我的工作布局。
12. 作为小窗口用户，我希望主侧边栏自动转为临时侧栏，以便主内容仍然可用。
13. 作为仓库用户，我希望点击仓库的展开箭头只展开 Review，而点击仓库名称打开仓库概览，以便导航行为可预测。
14. 作为仓库用户，我希望仓库概览显示仓库名称和完整路径，以便确认正在使用具体哪个本地 clone。
15. 作为仓库用户，我希望通过 Pull Requests、本地分支和 Review 历史三个标签进入不同任务，以便页面不会堆叠多个长列表。
16. 作为 Pull Request 审核者，我希望仓库概览默认打开 Pull Requests 标签，以便优先进入常见工作流。
17. 作为 Pull Request 审核者，我希望看到 Pull Request 标题、编号、作者、草稿和生命周期状态，以便选择正确目标。
18. 作为 Pull Request 审核者，我希望未创建 Review 的条目显示“开始审核”，已有 Review 的条目显示“继续审核”，以便理解操作结果。
19. 作为 Pull Request 审核者，我希望同一个 Pull Request 始终复用已有 Review，以便历史不会被重复记录分散。
20. 作为离线用户，我希望仍能看到并打开以前创建过的 Pull Request Review，以便继续阅读本地 Snapshot。
21. 作为离线用户，我希望界面明确说明在线 Pull Request 信息可能不是最新，以便正确理解缓存内容。
22. 作为在线用户，我希望仓库页面先显示本地保存的 Review，再在后台刷新 GitHub 信息，以便网络不会阻塞页面。
23. 作为在线用户，我希望 Pull Request 后台刷新失败时保留现有内容，以便局部失败不会清空可用数据。
24. 作为公开仓库用户，我希望未登录时仍能匿名发现公开 Pull Request，以便不被强制授权。
25. 作为私有仓库用户，我希望从明确入口连接 GitHub，以便访问授权范围内的 Pull Request。
26. 作为 GitHub 用户，我希望 GitHub App 安装、Device Flow 和退出登录集中在侧边栏底部入口，以便授权流程不占据仓库主页面。
27. 作为 GitHub 用户，我希望系统不自动弹出授权流程，以便只有主动连接时才离开当前任务。
28. 作为本地分支审核者，我希望 Base 默认使用仓库默认分支，Head 默认使用当前检出分支，以便减少常规选择步骤。
29. 作为本地分支审核者，我希望 Base 与 Head 相同时无法开始 Review，以便避免无意义比较。
30. 作为本地分支审核者，我希望已有相同 Branch Selection 时显示“继续审核”，以便复用已有 Review。
31. 作为本地分支审核者，我希望能够选择其他本地或远程跟踪分支，以便审核非默认分支组合。
32. 作为首次打开 Repository Entry 的用户，我希望在进入仓库前看到 Workspace Trust 风险说明，以便理解 checkout、hooks、filters 和 Git LFS 风险。
33. 作为已经授予信任的用户，我希望后续直接进入仓库且不再显示“已信任”状态，以便安全确认不成为持续噪音。
34. 作为选择了非 Git 文件夹的用户，我希望停留在原页面并看到明确错误，以便立即改正选择。
35. 作为 Review 阅读者，我希望打开 Review 后自动展示按路径排序的第一个 File Change，以便主区域不会空白。
36. 作为 Review 阅读者，我希望左侧文件树显示目录结构、文件状态和变更数量，以便理解变更范围。
37. 作为 Review 阅读者，我希望当前文件在文件树中高亮，以便保持位置感。
38. 作为 Review 阅读者，我希望调整文件树宽度，以便在长路径和大 Diff 之间取得平衡。
39. 作为 Review 阅读者，我希望文件树宽度只作为本地 UI 偏好保存，以便它不污染 Review 数据。
40. 作为 Review 阅读者，我希望第一版文件树保持简单且不显示搜索和筛选控件，以便减少不必要复杂度。
41. 作为 Review 阅读者，我希望在宽窗口中默认使用双栏 Diff，以便直接比较变更前后内容。
42. 作为 Review 阅读者，我希望切换单栏和双栏 Diff，以便适应文件类型和窗口尺寸。
43. 作为 Review 阅读者，我希望应用记住 Diff 布局偏好，以便维持个人阅读习惯。
44. 作为窄窗口用户，我希望界面临时使用单栏 Diff 且不覆盖桌面偏好，以便布局仍然可读。
45. 作为 Review 阅读者，我希望二进制、Symlink 和 Submodule 使用专用信息视图，以便不会看到空白或错误的文本 Diff。
46. 作为 Review 阅读者，我希望没有 File Change 时看到明确空状态，以便知道比较成功但没有差异。
47. 作为 Pull Request 阅读者，我希望 Review 顶栏显示标题、编号、状态和 Base 到 Head，以便快速确认审核对象。
48. 作为 Review 阅读者，我希望通过右侧概览面板查看详细元数据，以便详情不会长期压缩 Diff。
49. 作为 Pull Request 阅读者，我希望概览面板展示描述、作者、状态、完整 revisions 和 GitHub 链接，以便获得协作背景。
50. 作为本地分支审核者，我希望概览面板展示分支和 revision 信息，以便 Pull Request 与分支 Review 使用一致界面。
51. 作为安全敏感用户，我希望 Pull Request 描述禁止原始 HTML 和远程图片，以便查看描述不会静默加载外部内容。
52. 作为 Pull Request 阅读者，我希望外部链接由系统浏览器打开，以便 Narrant 不承担内嵌网页浏览。
53. 作为 Review 阅读者，我希望打开已有 Review 后在后台自动检查更新，以便及时知道 Selection 是否移动。
54. 作为 Review 阅读者，我希望自动检查不阻塞当前 Snapshot 的阅读，以便网络或 Git 操作不会中断工作。
55. 作为 Review 阅读者，我希望只有点击“刷新审核”才切换 Snapshot，以便正在阅读的内容不会静默变化。
56. 作为 Review 阅读者，我希望更新提示展示当前与可用短 revision，以便理解刷新影响。
57. 作为 Review 阅读者，我希望点击“刷新审核”后直接执行而不再二次确认，以便明确操作不会受到重复打断。
58. 作为 Review 阅读者，我希望刷新失败时继续保留旧 Snapshot 和 Diff，以便失败不会清空当前工作。
59. 作为 Review 阅读者，我希望刷新成功后自动打开新 Snapshot 的第一个 File Change，以便立即进入更新后的内容。
60. 作为同时处理多个 Review 的用户，我希望点击另一条 Review 时直接切换，以便不被无数据丢失风险的确认框打断。
61. 作为同时处理多个 Review 的用户，我希望新 Review 打开失败时继续保留当前 Review，以便错误不会让我落入空白页面。
62. 作为离开 Review 的用户，我希望应用自动释放临时 Review Workspace，以便本地资源得到及时清理。
63. 作为需要明确结束当前 Review 的用户，我希望在更多菜单中使用“关闭审核”，以便释放 Workspace 而不删除历史。
64. 作为仓库文件夹被移动或删除的用户，我希望侧边栏保留 Repository Entry 和历史 Review 名称，以便知道原有记录仍存在。
65. 作为仓库文件夹不可用的用户，我希望仓库页面展示原路径和不可用原因，以便理解为何不能打开 Review。
66. 作为执行耗时操作的用户，我希望加载反馈只出现在受影响区域，以便应用其他部分保持可用。
67. 作为执行后台刷新的用户，我希望已有列表和旧 Diff 保持可见，以便内容不会因刷新闪烁。
68. 作为遇到短暂失败的用户，我希望通过 Toast 收到无需立即处理的错误，以便不被模态弹窗打断。
69. 作为遇到输入错误的用户，我希望错误显示在对应输入附近，以便知道需要修正什么。
70. 作为遇到内容加载失败的用户，我希望错误显示在对应内容区域并提供恢复动作，以便错误具有上下文。
71. 作为简体中文用户，我希望应用根据系统语言显示中文，以便自然使用产品。
72. 作为英文用户，我希望应用根据系统语言显示英文，以便不必阅读中文界面。
73. 作为系统语言不受支持的用户，我希望应用稳定回退到简体中文，以便不会看到翻译 key 或空白文案。
74. 作为离线用户，我希望翻译资源随安装包提供，以便启动和导航不依赖网络。
75. 作为使用辅助技术的用户，我希望可见文案、ARIA label、Tooltip、Toast 和 Dialog 一致翻译，以便完整理解界面。
76. 作为用户，我希望日期和相对时间符合当前界面语言，以便最近 Review 的时间容易理解。
77. 作为桌面用户，我希望应用在至少 900×600 的窗口中保持可用，以便较小屏幕仍能完成审核。
78. 作为明暗主题用户，我希望 Narrant 跟随系统主题且 Diff 与外围界面协调，以便长时间阅读保持舒适。

## Implementation Decisions

- 应用采用三个主视图：最近 Review 首页、Repository Entry 概览和 Review 阅读器。
- 不设置横跨全窗口的全局顶栏。品牌区位于主侧边栏顶部，GitHub 连接入口位于底部，每个主视图拥有自己的页面级标题栏。
- 三个主视图共享同一个应用外壳，不引入 URL Router；主侧边栏承担主视图切换。点击品牌区进入首页，点击 Repository Entry 名称进入仓库概览，点击 Review 条目进入 Review 阅读器。
- 当前主视图在侧边栏中只对应一个选中项：首页选中品牌区，仓库概览选中 Repository Entry 名称，Review 阅读器选中所属 Repository Entry 下的 Review 条目并自动滚动到、展开所属 Repository Entry，但不同时选中 Repository Entry 名称。“最近审核”仅作为跨仓库快捷入口，其中的 Review 条目永不显示选中态；从这里打开 Review 后，选中态落到所属 Repository Entry 下的对应条目。不额外设置导航条或面包屑。
- 主侧边栏默认宽度约 280px，可拖动调整、折叠并保存前端偏好。拖动期间宽度必须逐帧跟随指针，临时关闭宽度过渡并禁止页面文本选择；折叠或展开仍可保留过渡。折叠后侧边栏宽度为零，不保留仅含展开按钮的图标窄栏。小窗口下使用临时侧栏。
- 主视图切换不重新挂载或重置主侧边栏；其宽度、开合状态和各 Repository Entry 的历史展开状态在视图间保持，并从前端本地偏好恢复。小窗口的临时侧栏在点击品牌区、Repository Entry 名称或 Review 条目后都保持打开，只由用户显式关闭。
- 主侧边栏使用唯一的展开/收起按钮：macOS 将按钮放在红黄绿窗口控件右侧的标题栏区域，Windows 和 Linux 将按钮放在主内容顶部工具区；按钮不随侧边栏一起隐藏。
- 主侧边栏的“最近审核”默认显示跨 Repository Entry 最近 5 条 Review；记录超过 5 条时提供“查看全部”，展开后显示最近 10 条。
- “最近审核”展开到 10 条后提供收起操作，恢复为最近 5 条。
- 首页主内容不复用侧边栏的折叠状态，始终直接显示最近 10 条 Review。
- 每个 Repository Entry 首次展开显示最近 10 条 Review；超过 10 条时提供“查看全部”，展开后提供“收起到最近 10 条”。
- 点击 Repository Entry 的展开控件只控制历史列表；点击名称打开 Repository Entry 概览。
- 多个 Repository Entry 可以同时展开，各自的展开状态作为前端本地偏好保存并在应用重启后恢复；打开 Repository Entry 概览不隐式改变其展开状态。
- 首次显示仓库列表时，当前 Repository Entry 自动展开，其他 Repository Entry 默认收起。此后以各仓库的持久化展开状态为准；从“最近审核”打开 Review 时自动展开其所属 Repository Entry，并保存该状态。
- Repository Entry 保持稳定的添加顺序，新添加的仓库位于前端；打开 Repository Entry 或其任一 Review 都不改变仓库之间的排列位置。当前 Repository Entry 仅显示选中态，不在列表中创建固定副本。
- Review 按最近打开时间降序排列；打开已有 Review 或创建并打开新 Review 都会更新其最近打开时间，使当前 Review 自然进入所属 Repository Entry 的列表顶部并成为选中项，不为旧 Review 创建固定副本或强制切换“查看全部”状态。
- 再次点击当前已选中的 Repository Entry 或 Review 不重新加载内容、重建 Review Workspace 或更新最近打开时间；内容刷新只由对应主视图中的明确操作触发。
- 从侧边栏打开另一个 Repository Entry 或 Review 时，原主视图和原选中项保持到新目标成功打开；目标条目局部显示加载状态并禁止重复触发。成功后原子切换主内容和选中态，失败则保留原视图与选中态。
- 侧边栏所有 Review 条目统一使用两行布局。Pull Request Review 第一行显示 `#编号 标题`，Branch Selection Review 第一行显示“本地分支”；相对时间作为独立列位于第一行右侧，并与侧边栏分隔线保留安全间距。第二行统一显示 Base/Head 分支关系。分支名溢出时截断，并通过 Tooltip 提供完整 ref；整行作为继续 Review 的点击目标，不显示额外操作按钮。
- 跨 Repository Entry 的“最近审核”条目在第一行先显示仓库名，再以布局间距分隔 Pull Request 的 `#编号 标题` 或“本地分支”；不使用点号或其他文本分隔符。第二行继续显示 Base/Head 分支关系。
- 没有 Review 的首次使用首页展示品牌、slogan“纳言：审好代码，睡安稳觉”、产品说明和“打开仓库文件夹”主按钮。
- 已有 Review 的正常首页使用“最近审核”作为标题，不重复展示完整品牌和 slogan；“打开仓库文件夹”位于同一标题行右侧。
- “打开仓库文件夹”使用系统原生目录选择器，不把手工路径输入作为主要交互。
- 选择目录后依次验证 Git 仓库、复用或注册 Repository Entry、检查 Workspace Trust，并仅在必要时显示风险 Dialog。
- 已经获得 Workspace Trust 的 Repository Entry 直接打开，不持续显示“已信任”状态。
- Repository Entry 概览使用 `Pull Requests`、`本地分支`、`审核历史` 三个标签，默认选择 Pull Requests。
- Pull Request 列表把未创建 Review 与已有 Review 分别标记为“开始审核”和“继续审核”。相同 Pull Request Selection 继续复用已有 Review。
- Pull Request 标签先展示本地持久化的 Pull Request Review 元数据，再后台发现 GitHub Pull Request。尚未创建 Review 的 Pull Request 不做离线列表缓存。
- GitHub 不可用时，已保存的 Pull Request Review 仍留在 Pull Requests 标签中，并以低干扰提示说明元数据可能不是最新。
- Pull Request 空状态区分未连接 GitHub、确实没有 Pull Request 和 GitHub 请求失败；连接 GitHub 必须由用户主动触发。
- GitHub App 安装、Device Flow、复制授权信息和退出登录进入侧边栏底部的 GitHub 连接界面，不长期占用 Repository Entry 概览。
- 本地分支标签默认使用仓库默认分支作为 Base Ref、当前检出分支作为 Head Ref。需要在应用边界提供这两项信息，而不是继续依赖分支列表顺序。
- Base Ref 与 Head Ref 相同时禁止开始 Review。相同 Branch Selection 显示“继续审核”。
- Review 阅读器由紧凑顶栏、可调整宽度的文件树和 Diff 主区域组成。
- 第一版文件树不提供搜索、状态筛选、已读或未读状态；显示目录、File Change 状态、总数和当前选择。
- 打开或刷新 Review 后，按稳定路径顺序自动选择第一个 File Change；零变更显示明确空状态。
- 文件树宽度、主侧边栏宽度和折叠状态、Diff 单双栏偏好只存于前端本地偏好，不进入 Rust 领域模型或 SQLite。
- Diff 在宽窗口中默认双栏，并允许切换为单栏；窄窗口可临时使用单栏而不覆盖保存偏好。
- 二进制、Symlink 与 Submodule File Change 使用专用内容状态，不实例化无意义的文本 Diff。
- Pull Request 与本地分支 Review 共用 Review 概览 Sheet。Sheet 默认关闭，通过 Review 顶栏打开。
- Pull Request 概览展示描述、作者、状态、Base/Head、完整 revisions 和 GitHub 链接；本地分支概览展示分支与 revisions。
- 侧边栏 Review 条目、首页最近审核、Repository Entry 概览、审核历史、Review 顶栏和概览 Sheet 中的 Base/Head 分支关系，统一显示为“Base 分支名 Badge、向左箭头图标、Head 分支名 Badge”。不额外显示 `Base`、`Head` 角色文字，不使用文本箭头或点号分隔；辅助技术文案明确读作“将 Head 分支合入 Base 分支”。
- Base 与 Head 使用相同、具有清晰边框和背景对比的中性 Badge 样式，不用颜色区分角色或暗示状态；悬停不得降低其对比度，当前 Review 由条目整体的选中背景表达。
- revision 之间的变化继续使用从旧 revision 指向新 revision 的向右箭头，避免把版本演进误表达为分支合入关系。
- Pull Request 描述使用安全 Markdown：跳过原始 HTML、阻止远程图片自动加载，HTTP/HTTPS 链接交给系统浏览器。
- 打开已有 Review 后后台自动检查 Selection 更新，但不自动刷新 Review Snapshot。
- “刷新审核”按钮本身构成明确确认，不再弹出二次 Dialog。刷新期间保留当前 Snapshot；成功后原子切换，失败后保留旧 Snapshot。
- 点击另一条 Review 直接切换；切换失败保留当前 Review。离开或切换成功后释放旧 Review Workspace。
- 页面导航自动关闭当前 Review 的运行时资源；更多菜单保留“关闭审核”，执行后返回所属 Repository Entry 的审核历史，不删除 Review 或 Snapshot。从 Review 阅读器点击所属 Repository Entry 名称也返回其“审核历史”标签并定位刚离开的 Review；从首页或其他 Repository Entry 直接打开仓库概览时仍默认进入 `Pull Requests`。
- 不可用 Repository Entry 继续显示在主侧边栏中，允许展开历史名称；主区域显示原路径和不可用原因。重新定位和删除不在本规格范围。
- 错误按范围反馈：短暂且无需立即处理的错误使用 shadcn Sonner；输入错误就地展示；阻断内容区域的错误在区域内展示；风险确认和用户选择使用 Dialog。导航失败若没有合适的就地承载区域，使用 Sonner 错误通知，且不改变当前主视图或选中态。
- 加载按范围反馈：首页和首次列表使用 Skeleton；后台列表刷新保留内容并显示局部进度；File Change 读取只占用 Diff 区域；Snapshot 刷新保留旧 Diff。
- 快速操作避免闪烁；只有持续约一秒的操作才增加文字说明。正常成功操作不普遍使用 Toast。
- UI 以中文为主，保留 Pull Request、Diff、Base、Head 和 GitHub 等约定术语。代码和领域文档继续使用 `CONTEXT.md` 的规范英文术语。
- 用户界面中的 Review、Repository Entry、Workspace Trust 和 Review Snapshot 分别显示为“审核”“仓库”“仓库信任”和“审核版本”。
- 引入 `i18next` 与 `react-i18next`。第一阶段支持 `zh-CN` 和 `en`，以 `zh-CN` 作为回退语言。
- 语言优先级为已保存的显式偏好、Tauri 系统 locale、`zh-CN` 回退。第一阶段不提供手动语言切换入口，但基础设施允许后续增加。
- 使用白名单规范化 BCP-47 locale。翻译资源随应用打包，并按 `common`、`catalog`、`review`、`github` namespace 加载。
- 使用原生 `Intl.DateTimeFormat` 与 `Intl.RelativeTimeFormat` 格式化绝对和相对时间。
- 翻译资源使用 TypeScript 常量和 i18next 类型扩展检查 message key。ARIA、Tooltip、空状态、Toast、Dialog 和恢复动作全部纳入翻译资源。
- Git 与 Rust 原始错误作为不可翻译技术详情；面向用户的摘要使用稳定、可翻译的错误类别。
- UI 优先使用现有 shadcn `base-maia` preset、taupe 色系、Remix Icon、主题 token、圆角和组件尺寸。
- 需要的通用能力优先安装和使用 shadcn 的 Sidebar、Resizable、Tabs、Sheet、Dialog、Dropdown Menu、Tooltip、Skeleton、Badge 和 Sonner；只有默认组件无法满足领域交互时才组合或新增组件。
- 视觉采用克制的桌面开发工具风格。品牌绿色仅用于主操作、选中和焦点；内容区域使用中性色、细边框和有限卡片。
- 首页和首次空状态使用舒展密度，Repository Entry 概览使用中等密度，主侧边栏、文件树和 Review 顶栏使用紧凑密度。
- 标题可使用现有 Oxanium 品牌字体，正文使用系统字体，代码与 revision 使用 JetBrains Mono。
- File Change 状态使用一致语义颜色：新增绿色、删除红色、修改橙色或黄色、重命名蓝色，并同时提供文字或图标，不能仅依赖颜色。
- 最小可用窗口目标为 900×600。1200px 以上同时展示主侧边栏、文件树和双栏 Diff；900–1199px 默认收起主侧边栏并允许 Diff 临时单栏。
- 第一版不增加产品级键盘快捷键，但所有 shadcn/Base UI 控件、文件树、菜单和 Dialog 必须保持标准键盘与焦点可访问性。
- 主视图切换不额外编排键盘焦点，沿用按钮和侧边栏组件的标准焦点行为；为显示当前 Review 而自动滚动侧边栏时不主动抢占焦点。
- 本规格不改变 Review、Review Selection、Review Snapshot、Repository Entry 或 Workspace Trust 的领域关系，也不改变只有一个 Review 同时打开的约束。

## Testing Decisions

- 测试只观察用户可见行为、Tauri 命令边界和持久化结果，不断言 React 内部 state、组件实现细节、Tailwind class 或 shadcn 内部结构。
- 主要测试 seam 是通过 WebdriverIO 驱动真实 Tauri 应用和系统 WebView，沿用现有 Repository Catalog、本地分支比较和 GitHub mock server E2E 基础设施。
- 真实 Tauri E2E 覆盖首次启动、原生目录选择、Workspace Trust、Repository Entry 概览、开始或继续 Review、自动选择 File Change、Diff 展示、显式刷新、Review 切换、Workspace 释放和应用重启恢复。
- E2E 至少覆盖一条中文核心路径和一条英文核心路径；测试环境显式固定 locale，不依赖 CI 主机语言。
- React 外部行为测试作为辅助 seam，通过 Testing Library 和 mock Tauri IPC 覆盖大量状态组合，继续使用 accessible role、name 和可见文案查询。
- React 测试覆盖侧边栏最近 5 条、展开到最近 10 条、Repository Entry 最近 10 条、查看全部、收起、主视图导航、Tabs、Sheet、Dialog、Toast、Skeleton 和空状态。
- 主视图导航测试验证三个主视图共用常驻侧边栏，品牌区、Repository Entry 和 Review 可切换对应内容，仓库树中只有当前项高亮且“最近审核”永不高亮，打开 Review 后所属 Repository Entry 展开、选中项可见且 Review 按更新后的最近打开时间排序。同时覆盖异步加载、打开失败保留原视图、重复点击不重载，以及小窗口切换后临时侧栏保持打开。测试不断言 `AppShell`、`MainSidebar` 或主视图状态的内部组件结构。
- React 测试覆盖 Pull Request 离线缓存、局部 GitHub 失败、不可用 Repository Entry、Review 切换失败保留当前内容，以及不同错误作用域。
- React 测试覆盖文件树自动选择、零变更、特殊 File Change、单栏与双栏切换、响应式降级和前端偏好恢复。
- i18n 单元测试覆盖 locale 规范化、显式偏好优先级、未知语言回退、namespace loader 白名单、绝对时间和相对时间格式。
- 每个 i18n 组件测试创建独立 i18next 实例并注入同步内存资源，避免全局实例在并行测试之间泄漏。
- 中文测试显式使用 `zh-CN`；首页、Repository Entry 空状态和 Review 更新至少各有英文 smoke test。
- i18n 测试覆盖可见文案以外的 ARIA label、Tooltip、Toast、Dialog 和恢复动作，确保辅助技术接收到同一语言。
- 新增的系统 locale、默认分支和当前分支应用边界需要 Rust 契约测试；现有 Review、Snapshot、Workspace 和 SQLite 行为继续由已有 Rust 测试覆盖，不因 UI 重构重复测试内部实现。
- Snapshot 更新测试必须证明自动检查不改变当前 Snapshot、失败刷新保留旧 Snapshot、成功刷新后数据库、File Changes 和 Review Workspace 一致。
- 安全 Markdown 测试必须证明原始 HTML 不执行、远程图片不请求、外部链接交给系统浏览器。
- 可访问性测试覆盖焦点进入和返回、Dialog 焦点约束、Sheet 关闭后的焦点恢复、非颜色状态表达以及 900×600 下的关键操作可达性。
- 构建和静态检查必须验证翻译 key 类型、TypeScript、ESLint 和现有测试套件；翻译资源必须离线可用。

## Out of Scope

- GitHub conversation comments、Review comments、Review submission 或任何 GitHub 写操作。
- AI 代码分析、AI Findings、模型集成或报告生成。
- Language Server 安装、索引、代码导航或符号理解。
- Review Snapshot 历史浏览 UI。
- Repository Entry、Review 或 Snapshot 删除 UI。
- Workspace Trust 撤销 UI。
- 不可用 Repository Entry 的重新定位 UI。
- 多个同时打开的 Review、标签页式 Review 或保留多个 Review Workspace。
- 文件树搜索、状态筛选、已读或未读状态、隐藏已查看文件。
- 产品级键盘快捷键和快捷键自定义。
- 手动语言选择器、独立设置页、繁体中文或简体中文与英文之外的翻译。
- 远程翻译资源、翻译管理云服务或第一版强制引入翻译供应商工作流。
- Pull Request 完整发现列表的离线缓存；离线只保证已经创建 Review 的 Pull Request 元数据可见。
- Pull Request 描述中的远程图片代理、图片白名单或内嵌网页浏览。
- 仓库路径手工输入作为主要流程。
- 手机或触摸优先布局。
- 任意 revision expression、tag 或对象 ID 选择。
- 对 GitHub Files changed 历史比较锚点的精确复刻。
- 改变现有 Core Git、SQLite、Review Snapshot 或 Review Workspace 的领域与架构决策。

## Further Notes

- 国际化技术调研见
  [Narrant 前端国际化方案调研](../research/frontend-i18n.md)。
- 本规格是整体产品体验目标，应拆分为若干可独立交付的垂直切片；每个切片都应保持至少一条从用户操作到真实应用边界的可运行路径。
- UI 重构应优先建立应用框架、i18n 与首页垂直切片，再迁移 Repository Entry 概览和 Review 阅读器，避免在一个超大改动中同时替换所有界面。
- 设计稿和实现不得把 Review Workspace 表现为用户可编辑的普通 checkout。
- 用户生成的标题、分支名、路径、Pull Request 描述和 Git/Rust 技术详情保持原文，不属于翻译资源。
- 本规格没有形成新的领域术语，也不修改根目录 `CONTEXT.md`。
- `i18next` 是常规且可替换的前端基础设施选择，不满足“未来读者会对该决定感到意外”的 ADR 条件，因此不新增 ADR。
