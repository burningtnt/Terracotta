# HTTP API 接入指南

Terracotta 内嵌了一个 HTTP 服务，同时托管 Web 控制界面与控制 API。第三方程序（如 HMCL 等启动器）可以通过该接口创建房间、加入房间、读取房间状态，实现与陶瓦联机的深度集成。

> 本文档基于版本 `v0.4.2` 源码整理。

## 运行模型

- HTTP 服务仅监听本机回环地址（`127.0.0.1`），端口由操作系统随机分配。
- 接口无鉴权，任何本地进程均可调用；请勿将端口暴露到外部网络。
- 浏览器跨域调用不可用（服务未返回 CORS 头），请在本地后端 / 启动器进程内发起请求。

### 端口发现

| 方式 | 说明 |
| --- | --- |
| `--hmcl <file>` 启动参数 | 以集成模式启动：`terracotta --hmcl <file>`。HTTP 服务就绪后，端口会以 JSON `{"port": <端口>}` 的形式写入 `<file>`——实际先落盘 `<file>.tmp`、再改名为 `<file>`，保证读取方任意时刻拿到的都是完整内容。Windows 下 `--hmcl` 进程仅作为引导：fork 出子进程作为真正的工作进程，主进程则等待 `<file>` 出现，最长 8 秒，超时即报错退出。 |
| 锁文件 `terracotta.lock` | 位于数据目录：macOS 为 `~/terracotta/`，其他平台为系统临时目录下的 `terracotta/`（如 Windows 的 `%TEMP%\terracotta`、Linux 的 `/tmp/terracotta`）。内容为 2 字节大端端口号。若已有实例在运行，新进程会作为「附属进程」直接复用该端口。 |

## API 一览

以下 Base URL 均为 `http://127.0.0.1:{端口}`，所有接口均为 `GET`。

### GET /meta

返回版本与平台信息：

```json
{
  "version": "v0.4.2",
  "compile_timestamp": "1750000000000",
  "easytier_version": "v2.5.0-terracotta.2",
  "yggdrasil_port": 13448,
  "target_tuple": "x86_64-pc-windows-msvc",
  "target_arch": "x86_64",
  "target_vendor": "pc",
  "target_os": "windows",
  "target_env": "msvc"
}
```

| 字段 | 说明 |
| --- | --- |
| `version` | Terracotta 版本号 |
| `compile_timestamp` | 编译时间戳（毫秒，字符串形式的十进制数字） |
| `easytier_version` | 内置 EasyTier 版本 |
| `yggdrasil_port` | 本地脚手架服务端口（TCP 私有协议），普通集成无需使用 |
| `target_tuple` 及其余字段 | 编译目标平台信息，格式为 `架构-供应商-系统-环境` |

### 状态机：GET /state

查询当前房间状态。建议以约 500ms 间隔轮询。

每次状态变化（包括房间成员变动）都会使响应中的 `index` 单调递增。**轮询方应记录已见过的最大 `index`，忽略 `index` 未增大的响应**。

各状态的响应结构如下：

#### waiting（待机）

```json
{ "state": "waiting", "index": 1 }
```

#### host-scanning（房主：正在扫描本机 Minecraft 端口）

```json
{ "state": "host-scanning", "index": 2 }
```

需要房主已启动 Minecraft 并进入存档，且对局域网开放（扫描到本机服务器后进入下一状态）。

#### host-starting（房主：正在创建房间）

```json
{ "state": "host-starting", "index": 3, "room": "U/XXXX-XXXX-XXXX-XXXX" }
```

#### host-ok（房主：房间已就绪）

```json
{
  "state": "host-ok",
  "index": 4,
  "room": "U/XXXX-XXXX-XXXX-XXXX",
  "profile_index": 0,
  "profiles": [ { "machine_id": "...", "name": "...", "vendor": "...", "kind": "HOST" } ]
}
```

展示 `room` 邀请码给其他玩家。`profiles[0]` 恒为房主本人；玩家加入或超时退出（10 秒无心跳）时成员列表会更新并递增 `index`。

#### guest-connecting / guest-starting（加入方：连接中）

```json
{ "state": "guest-connecting", "index": 5, "room": "U/XXXX-XXXX-XXXX-XXXX" }
```

```json
{ "state": "guest-starting", "index": 6, "room": "U/XXXX-XXXX-XXXX-XXXX", "difficulty": "MEDIUM" }
```

`difficulty` 为 NAT 连接难度评估：`UNKNOWN` / `EASIEST` / `SIMPLE` / `MEDIUM` / `TOUGH`。

#### guest-ok（加入方：加入成功）

```json
{
  "state": "guest-ok",
  "index": 7,
  "url": "127.0.0.1",
  "profile_index": 0,
  "profiles": [ { "machine_id": "...", "name": "...", "vendor": "...", "kind": "LOCAL" } ]
}
```

`url` 为访问房内服务器的直连地址：目标端口为 `25565` 时为 `127.0.0.1`，否则为 `127.0.0.1:{端口}`。可将其作为多人游戏备用直连地址展示给加入方。

加入成功后陶瓦会在本地广播多人游戏服务器到局域网加入列表，加入方可直接加入。从此处加入时游戏会忽略正版验证。

#### exception（异常）

```json
{ "state": "exception", "index": 8, "type": 0 }
```

| `type` | 含义 |
| :-: | --- |
| 0 | 无法连接房主（房间已关闭或网络不稳定） |
| 1 | 与房主的连接中断 |
| 2 | 加入方 EasyTier 崩溃 |
| 3 | 房主 EasyTier 崩溃 |
| 4 | 房主端 Minecraft 服务器失联（房主退出存档，房间自动关闭） |
| 5 | 脚手架协议错误（请向开发者反馈） |

进入异常状态后需调用 `GET /state/ide` 才能回到待机。

### 成员信息（profiles）

`host-ok` 与 `guest-ok` 响应中的成员对象结构：

| 字段 | 说明 |
| --- | --- |
| `machine_id` | 设备唯一标识，可用于区分玩家 |
| `name` | 玩家显示名，未提供时房主为 `Terracotta Anonymous Host`、加入方为 `Terracotta Anonymous Guest` |
| `vendor` | 来源标识 |
| `kind` | `HOST`（房主）/ `LOCAL`（本机玩家）/ `GUEST`（其他加入者） |

### 控制接口

#### GET /state/ide

重置为待机（`waiting`）状态，返回 `200`。用于「返回主页」等操作。

#### GET /state/scanning

以房主身份开房：

```
GET /state/scanning?room={邀请码}&player={显示名}&public_nodes={节点}&public_nodes={节点}
```

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `room` | 否 | 使用指定房间码重新开房；省略时自动生成新房间 |
| `player` | 否 | 房主显示名 |
| `public_nodes` | 否 | 附加的 EasyTier 公共节点，可重复传入，会与内置公共节点合并 |

仅在当前处于 `waiting` 状态时生效；否则请求被静默忽略，仍返回 `200`。

#### GET /state/guesting

以加入方身份加入房间：

```
GET /state/guesting?room={邀请码}&player={显示名}&public_nodes={节点}
```

| 参数 | 必填 | 说明 |
| --- | :-: | --- |
| `room` | 是 | 房间邀请码 |
| `player` | 否 | 加入方显示名 |
| `public_nodes` | 否 | 同上 |

- 邀请码有效且当前处于 `waiting` 状态：返回 `200`；
- 邀请码格式错误或当前不在 `waiting` 状态：返回 `400 Bad Request`。

> **实现提示**：也可以发后即忘——不检查返回值，让轮询自然进入 `exception` 状态来反馈错误。

### 运维接口

| 接口 | 说明 |
| --- | --- |
| `GET /log?fetch=true` | 返回 `application.log` 日志文件内容。macOS 上不带 `fetch` 参数时会改为在访达中打开日志目录并返回 `204`。 |
| `GET /panic?peaceful=true` | 优雅退出 Terracotta 进程。不带参数将直接触发 panic，仅供调试。 |
| `GET /` | Web 控制界面主页。 |

## 房间邀请码格式

参见 [内部协议](internal-protocol.md)。

## 典型接入流程

以 HMCL 启动器集成为例：

1. 启动器启动陶瓦进程（HMCL 以 `terracotta --hmcl <path>` 启动，等待临时文件出现获取端口）。
2. （可选）调用 `GET /meta` 展示版本信息。
3. 启动器启动后台轮询线程，按双频率策略请求 `GET /state`：UI 活跃时约 500ms、后台闲置时约 15s。
4. 用户点击「创建房间」→ 调用 `GET /state/scanning?player={显示名}&public_nodes={节点}&public_nodes={节点}...`。随后轮询线程会自然收到 `host-ok` 状态，展示 `room` 邀请码与成员列表。
5. 用户点击「加入房间」→ 同样先拉取节点列表，再调用 `GET /state/guesting?room={邀请码}&player={显示名}&public_nodes={节点}...`。
6. `guest-ok` 后，将 `url` 作为多人游戏备用直连地址展示。
7. 用户返回主页 → 调用 `GET /state/ide`。
8. 需要退出陶瓦时 → 调用 `GET /panic?peaceful=true`。
9. 轮询连续 5 次重试均失败后，可视为陶瓦进程已退出或异常。
