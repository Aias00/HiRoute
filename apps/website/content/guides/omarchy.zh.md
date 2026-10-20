# 在 Omarchy 上使用 HiRoute

Omarchy 集成是可选功能。启用后，从 Omarchy 桌面启动的 Codex 和 Claude Code 会话经由 HiRoute 路由：真正需要订阅模型的轮次继续使用订阅，其余轮次交给 Plan 中的其他模型，让订阅额度用得更久。Omarchy 自带的 Agent 入口保持不变，HiRoute 只在旁边添加自己的入口。

启用后会添加：

- 应用启动器中的两个入口 **Codex · 难度路由** 和 **Claude Code · 难度路由**，以及一个打开控制台的 **HiRoute** 入口；
- Omarchy 状态栏右侧的 HiRoute 小组件及其面板；
- 关于信任建议、服务不可用和路由启动失败的桌面通知。

## 1. 准备

- 按 [Linux 无界面版安装](/docs/install-linux/) 安装 HiRoute 并启动服务。集成依赖这份当前用户安装。
- 按常规方式安装 Codex 和／或 Claude Code。
- 在 Omarchy 会话中的终端运行下面的命令。HiRoute 通过 `OMARCHY_PATH` 和 `omarchy` 命令识别 Omarchy；在其他环境中，启用会直接停止，不写入任何文件。

## 2. 启用集成

```sh
hiroute integrations omarchy enable --console yes
```

启用会为当前用户写入启动器入口、HiRoute 图标和 `hiroute.status` 状态栏小组件，再通过 Omarchy 自己的 `omarchy plugin` 命令把小组件加入状态栏。它不修改 Omarchy 的文件，也不会启动、停止或重启 HiRoute。如果要写入的位置已有一个不属于 HiRoute 的文件，启用会停止并指出该文件，而不会接管它。

尚未配置控制台时，`--console yes` 会配置一个仅本机访问的控制台；`--console no` 则跳过。如果控制台是这次启用时刚配置的，请重启一次服务，让它开始监听：

```sh
hiroute service restart --output json
```

通知默认开启。加上 `--notifications off` 可以在关闭通知的状态下启用。

随时可以查看状态：

```sh
hiroute integrations omarchy status
```

它会报告每个 HiRoute 所拥有文件的状态（`present`、`missing`、`changed` 或 `outdated`）、每个路由 Agent 的启动检查结果、控制台是否已配置并在监听，以及通知设置。

## 3. 选择要试用的模型

从应用启动器打开 **HiRoute**，或在状态栏面板中点 **打开 HiRoute**。启用后第一次打开时，控制台会进入试用快速开始，并已选好 Codex 和 Claude Code 作为目标：

- Codex 使用 HiRoute 的独立配置（profile）；
- Claude Code 使用独立模式。

两者都只影响经由 HiRoute 启动的会话。直接运行 `codex` 或 `claude`，以及它们的原生设置，都保持原样。选择要试用的模型并完成快速开始；最后一步会提示你使用这两个启动器入口。

## 4. 启动路由会话

在应用启动器中选择 **Codex · 难度路由** 或 **Claude Code · 难度路由**。入口会以 Omarchy 默认的交互参数启动 Agent（Codex 为 `--approve-for-me`，Claude Code 为 `--permission-mode auto`），实际执行：

```sh
hiroute integrations omarchy launch --agent codex -- --approve-for-me
```

会话的行为与 Omarchy 自带的 Agent 入口一致：

- 在同样窗口类名的终端中打开，你的窗口规则依然生效；
- 从主目录启动且 `~/Work` 存在时，在 `~/Work` 中启动；否则保留当前目录。

每个路由会话都使用 HiRoute 的独立连接。在终端中运行 `hiroute agent launch --agent codex -- <参数>` 会启动同样的路由会话，只是不带上述桌面约定。

> **账户边界。** 路由会话使用在 HiRoute 中配置的订阅连接，不跟随 Omarchy 当前账户。在 Omarchy 中切换账户，不会改变路由会话使用的账户。

## 5. 查看状态栏指示

小组件显示今天由订阅之外的模型处理的 token 占比，例如 `42.5%`。这是跨模型的近似值，不代表配额、费用或节省。部分请求没有记录用量时，提示中会注明“部分用量未知”。无法给出占比时，小组件显示短横线，提示说明原因：今日没有路由请求、未使用订阅来源、订阅来源的用量未知，或没有可比较的用量。

有待处理的信任建议时显示一个圆点。HiRoute 不可用或某个路由 Agent 无法启动时，小组件显示为警示色。

点击小组件打开面板：

- 每个路由 Agent 启动槽位背后的 Plan：Codex 的默认模型，以及 Claude Code 的 Opus、Sonnet 和 Haiku。每个 Plan 显示其信任档位与阈值；没有难度信任控制的 Plan 显示“无信任档位”，各分支设置不同的 Plan 显示“按分支设置”。**另外 N 个 Plan** 会在 HiRoute 中打开该 Agent 的完整 Plan 列表。
- 信任建议的数量，以及有几个 Plan 的建议暂时无法读取。
- 上面的账户边界说明。
- **打开 HiRoute**；**路由时间线**，在终端中用 `hiroute sessions timeline <SESSION_ID> --follow` 跟随最近的路由会话；以及 **开启／关闭通知**。

右键点击小组件可直接打开 HiRoute。小组件每分钟刷新一次，打开面板时也会刷新。如果没能及时读到当前数据，它会显示“HiRoute 暂不可用”，而不是继续显示旧数据。

官方安装脚本升级 HiRoute 时会刷新小组件文件。正在运行的 Omarchy shell 可能会继续使用已加载的旧版小组件，直到它重启。可以运行 `omarchy restart shell`，或注销后重新登录。

## 6. 通知

HiRoute 会在以下情况发送桌面通知：

- 每条新的信任建议通知一次；选择通知会在 HiRoute 中打开这条建议的依据；
- 服务每次从就绪变为不可用时通知一次；
- 路由启动未通过就绪检查时。

可以在面板中关闭通知，或运行：

```sh
hiroute integrations omarchy configure --notifications off
```

关闭通知时，尚未发送的通知也会一并丢弃。

## 7. 路由启动失败时

每个路由入口在启动 Agent 前都会检查就绪状态。检查未通过时（例如服务已停止、Agent 被 mise 升级后需要重新连接，或映射的 Plan 已不再发布），你会收到一条通知；终端会说明原因，并询问是否改为原生启动。

只有在你输入 `y` 之后才会原生启动。原生启动不经过路由，使用你平常的环境，也不会应用 Omarchy 按账户的选择。未经你确认，绝不会以未路由的方式启动。

## 8. 停用与卸载

```sh
hiroute integrations omarchy disable
```

停用会通过 Omarchy 从状态栏移除小组件，删除你没有改动过的 HiRoute 文件，保留并报告你编辑过的文件。之后随时可以重新启用。

[独立服务版卸载](/docs/install-linux/) 会先执行停用。停用失败时卸载会停止并说明原因，因此不会留下无人管理的集成文件。
