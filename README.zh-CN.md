# herdr-agent-quota

在 Herdr Agent 侧栏显示模型、上下文、提示词缓存用量和订阅额度。

[![CI](https://github.com/levi-qiao/herdr-agent-quota/actions/workflows/ci.yml/badge.svg)](https://github.com/levi-qiao/herdr-agent-quota/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[English](README.md)

<table>
<tr><th>packed（默认）</th><th>stacked</th></tr>
<tr>
<td valign="top"><img src="docs/screenshots/sidebar-packed.png" alt="拼接布局" width="284"></td>
<td valign="top"><img src="docs/screenshots/sidebar-stacked.png" alt="分行布局" width="177"></td>
</tr>
</table>

插件保留 Herdr 原生的机器／工作区／标签页行、自定义样式和 worktree 分组。
带品牌色的 provider/model 行就是 agent 身份，因此不再保留灰色的原生 `agent` 行，
避免 `grok` 叠在 `Grok/grok-4.6` 上面。按额度排序和低额度通知默认关闭。
空字段自动折叠，百分比可选择显示剩余或已用额度。

## 安装与升级

要求：**Herdr 0.9.0+**、`rust-toolchain.toml` 指定的 Rust 工具链、macOS 或 Linux，
以及受支持的 agent CLI。

```sh
git clone https://github.com/levi-qiao/herdr-agent-quota.git
cd herdr-agent-quota
./install.sh
```

只启用部分 agent：`./install.sh --agent claude,codex,omp`。
仅在需要加载新安装的 hook 或 Herdr integration 时，才需重启已经运行的 agent 会话。

在仓库目录升级：

```sh
git pull --ff-only
./install.sh
```

升级保留已有偏好，修复插件管理的配置，重新读取额度并自动恢复后台更新。
不需要删除缓存或管理 watcher 进程；Herdr 服务端连接变化后，watcher 会自动接管。

## 设置

按 `prefix+shift+q` 打开；若该快捷键已有其他用途，可运行：

```sh
herdr plugin pane open --plugin herdr-agent-quota --entrypoint settings --focus
```

<img src="docs/screenshots/settings.png" alt="Agent quota 设置" width="760">

| 设置 | 可选项 |
| --- | --- |
| Percentages | 剩余或已用比例；颜色始终表示剩余额度 |
| Layout | `packed` 合并相关字段，`stacked` 将字段分行显示 |
| Row gap | Agent 之间保留零行或一行空白 |
| Watch interval | 30 秒–1 小时，默认 60 秒 |
| Fields | 主题、模型、缓存、TTL、上下文、短期／长期额度 |
| Brand colors | 开启或关闭品牌色 |
| Agent order | Herdr 默认排序，或剩余额度最少的优先 |
| Low quota alert | 关闭，或设置 1%–100% 的提醒阈值 |
| Agents | Claude、Codex、Grok、Agy、OpenCode、Pi、OMP、Devin |

方向键或空格修改，`a` 应用，`q` 关闭。脚本配置选项见 `./install.sh --help`。

### 原生 Codex 账号目录

原生 Codex 窗格通过 Herdr 提供的精确会话 UUID，与允许使用的 Codex 目录中的
rollout 首行记录匹配。未配置时，使用插件进程的 `CODEX_HOME`，未设置则使用
`~/.codex`。无法确认会话或账号身份时显示 `N/A`，不会借用其他目录的额度。

使用多个账号时，在插件的 `HERDR_PLUGIN_CONFIG_DIR` 下创建 `codex-homes` 文件，
内容为账号目录绝对路径的 JSON 数组。路径应由账号管理器的解析器提供。
此列表会替代默认目录，需包含所有要显示原生窗格额度的账号目录。
这是独立于设置弹窗的高级偏好。Herdr 插件 action 在服务端环境中运行，因此在
`herdr plugin action invoke` 命令外导出 `CODEX_HOME` 不会改变 action 的配置。

列表最多包含 32 个目录，JSON 文件上限为 64 KiB。相对路径、格式错误、目录不可读、
同一会话匹配多个目录，或会话链接越出所属目录时，均拒绝归属并显示身份不可用的 `N/A`。
`auth.json` 必须是普通文件；符号链接一律拒绝，即使目标是同一目录内的普通文件。
删除允许列表文件可恢复默认的单目录模式；完整卸载也会删除该文件。

每个目录独立使用 collector、缓存、刷新租约和 60 秒防抖。
共享同一目录且凭据未变的窗格共用一次请求。账号切换或令牌轮换不会改变存储路径：
每个规范化目录固定使用一个快照文件、一个刷新标记文件和一个租约文件。
快照和刷新标记记录不透明的凭据代次标记；代次变化后，不复用旧额度、旧窗口或旧防抖状态。
`auth.json` 的任何内容变化都会使缓存归属失效，即使组织账号 ID 未变。
这也包括常规令牌轮换，必须重新成功读取后才显示额度。
解析器要求使用文件认证的 ChatGPT 账号，不支持仅存于钥匙串的认证或 API key 认证。

低额度提醒按原生 Codex 目录和账号分别记忆状态。其他账号额度充足、窗格缺席或身份不可用，
都不会使低额度账号再次触发提醒；只有实际观测到该账号恢复至阈值以上，才允许下一次低额度提醒。
提醒身份在令牌轮换后保持稳定；其他 collector 保留原有的 provider 级提醒行为。

原生窗格运行期间，应让每个目录始终专用于同一个账号。在原目录内切换登录后，
会话与目录的映射无法识别已运行 CLI 内部保留的凭据；额度来自该目录当前的 app-server 账号。
会话 rollout 仅提供有界读取的本地模型和上下文诊断，不补充或替代账号额度。
请求失败时，仅在凭据未变的情况下保留最后一次有效快照；成功的 API 读数替换全部窗口，
未返回的窗口也会移除，不从旧快照或 rollout 补回。

源码验证流程见 [verify-herdr-agent-quota](docs/verify-herdr-agent-quota/SKILL.md)。

## 数据来源与边界

| Agent | 额度来源 | 归属依据 |
| --- | --- | --- |
| Codex | Codex app-server；5h 和／或 7d | 精确原生会话匹配唯一允许的账号目录 |
| Grok | CLI billing 接口；7d 或 30d | 当前 CLI 凭据 |
| Devin | CLI usage 接口；1d 和 7d | 当前 CLI 凭据 |
| Claude Code | StatusLine；5h 和 7d | 精确会话的观测 |
| Agy / Antigravity | StatusLine；5h 和 7d | 精确会话与可确认的模型额度池 |
| OpenCode | OpenCode Go usage 接口 | Go 凭据；确认的 PAYG 路由不显示订阅额度 |
| Pi | 规范 Codex collector 的额度 | 仅在记录的账号一致时复用 |
| OMP | `omp usage --json --provider <id>` | usage 账号与会话 credential pin 一致 |

额度窗口保留上游定义。模型、上下文和缓存数据优先来自已识别的会话。
`ttl≈` 表示估算的提示词缓存寿命，不保证实际过期时间。
主题提取只读取事件点名窗格的可见屏幕；内容滚走后保留已有主题。

所有受支持的工作中 agent 共用一个后台 watcher，请求间隔至少 60 秒，并在回合结束后
完成收尾刷新。OMP 另有自身的五分钟 usage 缓存。共享已确认额度来源的闲置窗格会收到同一读数。

原生 Codex 将每个窗格的精确会话归属到唯一允许的账号目录，并使用该目录的当前账号。
Grok 和 Devin collector 使用插件环境中的当前 CLI 凭据，不为每个窗格分别识别账号。
Claude/Agy 没有可靠的服务账号 ID，因此不跨会话共享观测值。
账号或模型额度池无法确认时不猜测数字。请求失败保留同一账号最后一次已确认的读数，
不会把失败解释为零用量；原生 Codex 还要求凭据未变，才能保留该读数。

## 常见问题

| 现象 | 检查 |
| --- | --- |
| 缺少会话数据 | 运行 `herdr integration status`，安装缺失项后重启对应 agent |
| Claude/Agy 缺少额度 | 发送一轮消息，让该会话的 StatusLine 产生观测 |
| OMP 缺少额度 | 检查 `omp usage --json --redact --provider <id>` |
| Devin 缺少额度 | 检查 CLI 登录；使用自定义路径时检查 `DEVIN_CREDENTIALS_FILE` |
| 缺少侧栏行 | 运行下面的 configure action 修复插件配置 |
| packed 内容被截断 | 选择 `stacked` |

```sh
herdr plugin action invoke refresh --plugin herdr-agent-quota
herdr plugin action invoke configure --plugin herdr-agent-quota
```

完整卸载使用 `./uninstall.sh`，只移除部分 agent 使用 `./uninstall.sh --agent grok`。
配置修改可恢复，用户自己的设置与其他 agent 不受影响。

## 参与开发

开发与验证见 [CONTRIBUTING.md](CONTRIBUTING.md)，数据处理及漏洞报告见
[SECURITY.md](SECURITY.md)，版本变更见 [CHANGELOG.md](CHANGELOG.md)。
历史调研索引见 [docs/README.md](docs/README.md)。

## 许可证

[MIT](LICENSE)。本项目与 Herdr 及受支持的 AI 供应商无隶属关系。
