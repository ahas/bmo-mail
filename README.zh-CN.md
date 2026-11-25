[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **简体中文**

# bmo-mail

作为一个拥有独立 API 的服务的邮件：每个登录账户都有自己的邮箱，邮箱从页面上添加，并通过针对每种账户类型的
适配器读取、撰写和监视，首先支持的是 IMAP 和邮件提交（submission）。它自行完成列表的会话归并和分页，在邮件
到达时执行过滤规则，查找发件人的徽标，在账户的撤销时限内暂存已发送的邮件以便撤回，并通过自己的事件流告知
收到了什么、改变了什么。它不调用任何其他服务。它的托管工具 `bmo/mail_sent` 从某人自己电脑上的 Outlook，或借助可选的
页面令牌服务借出的令牌从 Microsoft Graph，读取此人某一天发送的邮件，途经代理服务（agency）、接收器、账户服务、
节点服务以及该页面令牌服务。编码工具（harness）通过其 MCP 服务器 `bmo-mail mcp` 和 `bmo-mail` 技能引入它；控制台则按
`docs/api.md` 的规定，通过服务随清单一同提供的 `client/` 中的包引入其页面。

## 测试

测试平台的 `personal-data` 检查负责执行个人数据规则（`PERSONAL-DATA.md`）。仓库包含以下测试：旧归档的导入
位于 `tests/import.rs`；账户位于 `tests/accounts.rs`，涵盖账户的路由、一个所有者的密钥绝不触及另一个所有者的
数据、密钥的封存以及账户文件的导入；断开连接时的监视器位于 `tests/watcher.rs`；POP3 账户位于 `tests/pop3.rs`，
以测试用的 POP3 服务器和提交服务器检验收件箱的列出、带附件邮件的读取、按 `UIDL` 识别新邮件，以及在存储中
保存为草稿的回复的发送；邮件服务器位于 `tests/servers.rs`，涵盖将服务器成为记录之前的账户按相同设置迁移到同一条服务器
记录、所有者通过测试用账户服务的委托链可达的服务器、账户在其中的选择，以及账户服务通过测试用提交服务器发送的
系统邮件；控制台接口位于
`tests/console.rs`，涵盖通过已 vendor 的 bmo-ui 版本自身的读取方式读取所提供的清单，以及控制台凭据和服务令牌
各自的作用范围；页面文案位于 `tests/words.rs`，确认页面写出的每个键要么属于其自己的语言包，要么是该版本的
`wording-keys` 模块所列出的键；托管工具位于 `tests/sent_mail.rs`，以工具宿主调用它的方式，针对代理服务、节点
服务、页面令牌服务和 Graph 的替身进行调用，检查涉及谁和哪台机器、两条通道的顺序、向每个上游请求时使用的
令牌、拒绝情况，以及工具文件夹中保存的模式。`cargo test` 会运行以上全部测试。控制台插件包的浏览器测试位于
`client/tests/`：页面在那里的一个小型 Nuxt 宿主中独立运行，由客户端的开发服务器提供，并在 Chromium 中用鼠标
操作，对接一个会保存页面所做更改的服务替身；`tests/browser/mail-drag.test.ts` 把列表行拖到邮件导航的文件夹和
标签上，把过滤规则拖到别的位置，并在重新加载页面后读取每个结果；
`tests/browser/mail-kind-actions.test.ts` 确认对不移动邮件的账户类型，在宽窗口、窄窗口和手机宽度下都不显示
移动邮件的操作；`tests/browser/mail-servers.test.ts` 在宽窗口和手机宽度下，连同连接测试保存个人自己的服务器，并用
从选择框中选出的服务器（包括新服务器）添加账户。包的检查在类型检查之后运行这些测试。其他行为
测试暂缓：等最小化的服务在新仓库上运行起来后，再在这里编写和运行；旧行为曾经遵守的内容由 `docs/inventory.md`
列出。构建检查是 `cargo check --all-targets` 和包的类型检查。

## 目录结构

| 目录 | 内容 |
|---|---|
| `src/` | 服务、其客户端、OpenAPI 文档和 MCP 服务器及其命令，是一个拆分为多个模块的 crate |
| `examples/` | `openapi`，按 `docs/openapi.json` 所保存的样子输出 OpenAPI 文档；`tool_schemas`，把托管工具的模式写入其文件夹 |
| `migrations/` | 存储，其表由这些语句按顺序创建 |
| `wording/` | 服务自身的文案，每种语言一个文件 |
| `plugin/` | 编码工具插件的技能 `skills/bmo-mail/` |
| `tools/` | 托管工具 `sent-mail/`：其清单、模式和使用手册 |
| `client/` | 控制台插件包：邮件页面，是一个独立的项目 |
| `deploy/` | 服务的用户单元 |
| `docs/` | 关于服务行为的规范文档，以及 OpenAPI 文档 |
| `tests/` | 现有的行为测试 |

根目录下有 Cargo 清单、`LICENSE`、`testbed.toml`、`install.toml`、`rustfmt.toml`、`CLAUDE.md`、`PERSONAL-DATA.md`、
`korean-ledger.json`、`settings.toml`，以及部署所提供的本地设置的示例 `settings.local.toml.example`。

## 控制台插件包

`client/` 以基于 bmo-ui 的控制台包的形式承载邮件页面。它通过 bmo-chat 的控制台接口（`docs/api.md`，
「The console plugin bundle」）遵守 bmo-ui 的控制台插件契约（bmo-ui 的 `docs/console-bundles.md`）：`bun run bundle`
使用 bmo-ui 的包构建工具，从 `client/console/` 构建模块、样式表、文案以及构建所基于的层，服务在其声明生成的
清单旁边提供这些文件。bmo-chat 固定的 bmo-ui 版本和 bmo-text 的 TypeScript 包放在 `client/vendor/` 下，供其自身
安装、类型检查和构建使用，因此克隆仓库后无需其他检出目录。请求和响应体的类型由 `client/shared/openapi.ts`
定义，该文件由 `bun run openapi` 从 `docs/openapi.json` 生成。包的检查是在 `client/` 内运行的 `bun run check`，
包括锁定的安装、类型检查和浏览器测试。

## 文档

| 文档 | 内容 |
|---|---|
| `docs/mail.md` | 邮件的功能：账户、监视器、读取、撰写、过滤、头像和页面 |
| `docs/api.md` | 路由、事件流、客户端、编码工具插件、服务自身的文案、存储、命令、设置和控制台插件包 |
| `docs/openapi.json` | 由服务自身的类型生成的 OpenAPI 3.1 契约 |
| `docs/inventory.md` | 本仓库从旧仓库中接收的内容，以及每组内容的数量和来源 |
| `PERSONAL-DATA.md` | 本仓库中绝不能出现的内容：人的姓名、地址、路径、设备或网络信息，房间成员或公司的信息，真实对话中的任何一行，以及保证这一点的检查 |

## 版本

版本属于整个套件，在构建时从 git 标签读取，健康检查、MCP 服务器和 `--version` 都以此版本作答；crate 的版本只是永不变动的占位值。
如 bmo-chat 的 `docs/conventions.md`「Versions」所述，版本只在所有者宣布的发布时变动：提交、部署或契约变更都
不会改变版本，部署以其提交区分。
