# Android API 接入指南

Terracotta 在 Android 平台以 JNI 静态库的形式发布，配套提供一个 `TerracottaAndroidAPI` Java 类作为接入层。启动器或联机应用只需将该 Java 文件拷贝进工程，并将 `libterracotta.so` 打包到 APK，即可直接调用陶瓦的房间创建、加入、状态查询与日志收集能力。

> 本文档基于版本 `v0.4.2` 源码整理。状态 JSON 的字段定义与 [HTTP API](http-api.md) 一致。

## 引入方式

1. 将 `ffi/TerracottaAndroidAPI.java` 拷贝到工程中。该类不依赖 Terracotta 其它 Java 代码，可以整体重定位到任意包名下——只需保持文件名不变，类加载时会通过 `net.burningtnt.terracotta.native_location` 系统属性自动告知原生层实际的 JNI 类路径。
2. 将为 Android 构建的 `libterracotta.so` 放入 `jniLibs/<abi>/` 目录。类加载时自动执行 `System.loadLibrary("terracotta")`，无需手动加载。
3. 准备好一个 `VpnService` 子类用于建立 VPN 连接（见下文「VpnService 接入」）。

类上所有方法均为静态方法且线程安全，可在任意线程并发调用。但 EasyTier 初始化期间，`getState` / `setWaiting` / `setScanning` / `setGuesting` 可能阻塞约 1 秒，建议在后台线程调用。

## 生命周期与初始化

`TerracottaAndroidAPI` 是进程级单例：`initialize` 只能成功调用一次，重复调用会抛出 `IllegalStateException`。调用前必须先准备：

- 一个 Android `Context`（用于定位 `filesDir`，陶瓦会在其下创建 `net.burningtnt.terracotta/` 目录，包含 EasyTier 数据目录 `rs/` 与日志文件 `application.log`）；
- 一个 `VpnServiceCallback` 回调，用于接收陶瓦发起的 VPN 建立请求。

```java
TerracottaAndroidAPI.Metadata metadata = TerracottaAndroidAPI.initialize(
    context,
    () -> {
        // 陶瓦请求建立 VpnService，此处应跳转到 VpnService 授权界面或直接建立连接
    }
);

Log.i("Terracotta", "version=" + metadata.getTerracottaVersion()
        + ", easytier=" + metadata.getEasyTierVersion()
        + ", compileTime=" + metadata.getTerracottaCompileTime());
```

`Metadata` 包含三个字段：陶瓦版本号、编译时间戳（毫秒，与 `System.currentTimeMillis()` 同基）、EasyTier 版本号。

初始化失败（目录无法创建、原生层启动失败等）会抛出 `RuntimeException`。

## 状态读取与切换

### 读取状态

`getState()` 返回一个 JSON 字符串，结构同 [HTTP API 的 `/state`](http-api.md)：

- `index` 字段单调递增，每次状态变化（含成员变动）都会自增。轮询方应记录已见过的最大 `index`，忽略未增大的响应。
- `state` 字段标识当前状态：`waiting` / `host-scanning` / `host-starting` / `host-ok` / `guest-connecting` / `guest-starting` / `guest-ok` / `exception`。

建议以约 500ms 间隔在后台线程轮询。

### 切换状态

| 方法 | 说明 |
| --- | --- |
| `setWaiting()` | 重置为待机状态，用于「返回主页」等操作。 |
| `setScanning(@Nullable String room, @Nullable String player)` | 以房主身份开房。`room` 传入已有邀请码可复用原房间，传 `null` 则新建；`player` 为房主显示名，传 `null` 使用默认值。仅在当前为 `waiting` 状态时生效，否则静默忽略。 |
| `setGuesting(String room, @Nullable String player)` | 以加入方身份加入房间。`room` 不可为空；邀请码格式错误、或当前不在 `waiting` 状态时返回 `false`，成功受理返回 `true`。 |
| `parseRoomCode(String room)` | 仅解析邀请码，不切换状态。返回 `RoomType.TERRACOTTA_LEGACY` / `PCL2CE` / `SCAFFOLDING`；格式无效返回 `null`。 |

除 `parseRoomCode` 外，以上方法都要求在 `initialize` 成功之后调用，否则抛出 `IllegalStateException`。

`parseRoomCode` 足够快，可在 UI 线程直接调用，但仍建议放到工作线程以防潜在的 UI 卡顿。

## VpnService 接入

Android 上 EasyTier 需要一块虚拟网卡（TUN 设备），只能通过系统的 `VpnService` 建立。Terracotta 采用「请求—响应」模式：

1. 原生层在需要 VPN 时，在 Rust 工作线程上回调 `initialize` 时传入的 `VpnServiceCallback.onStartVpnService()`。**该回调不在 UI 线程**，请不要在此直接操作界面；如需跳转授权页或启动 Service，请自行切换到主线程。
2. Java 层调用 `getPendingVpnServiceRequest()` 拿到一个 `VpnServiceRequest` 对象。该对象一次性：`startVpnService` 或 `reject` 任一调用后，`pendingRequest` 即被清空；下一次 VPN 请求会生成新的对象，需重新获取。
3. 在 **30 秒** 内，必须调用该对象的 `startVpnService(builder)` 或 `reject()`，二者其一：
   - 调 `startVpnService(builder)`：基于传入的 `VpnService.Builder` 调用 `establish()` 建立连接。底层 fd 通过 `ParcelFileDescriptor.getFd()` 共享给原生层（非 detach），因此返回的 `ParcelFileDescriptor` 必须由开发者持有，在 EasyTier 退出后由开发者 `close()`。
   - 调 `reject()`：告知原生层本次不建立 VPN。原生层会在日志中记录一条错误并继续运行，EasyTier 启动流程随后会因拿不到 TUN fd 而失败。
4. 若 30 秒内两者都未调用，`onVpnServiceStateChanged` 会抛出 `IllegalStateException`，由原生层捕获并记日志，效果等同于 `reject()`。

首次集成时，需先通过 `VpnService.prepare(context)` 检查用户是否已授权 VPN：返回非空 `Intent` 时，应先跳转到系统授权页，待用户同意后再触发陶瓦的状态切换（`setScanning` / `setGuesting`）。否则 `establish()` 会直接返回 `null`。

`VpnServiceRequest.startVpnService` 收到的 `Builder` 已经预配置了：

- 虚拟 IP 地址（`addAddress`，来自 EasyTier 分配的 IPv4）；
- 两个 DNS 服务器：`223.5.5.5`、`114.114.114.114`；
- 若干 CIDR 路由（`addRoute`）。

开发者可以在传入前继续定制（如 `addAllowedApplication`、MTU、Session 名等），但不要改动已配置的地址与路由，否则 EasyTier 无法正常工作。

完整示例：

```java
public final class TerracottaVpnService extends VpnService {
    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        VpnServiceRequest request = TerracottaAndroidAPI.getPendingVpnServiceRequest();

        Builder builder = new Builder()
                .setSession("Terracotta")
                .setMtu(1400);

        ParcelFileDescriptor tun;
        try {
            tun = request.startVpnService(builder);
        } catch (RuntimeException e) {
            // establish() 返回 null，通常是用户尚未授权 VPN
            request.reject();
            stopSelf();
            return START_NOT_STICKY;
        }

        // 由应用自行持有 tun；EasyTier 退出后调用 tun.close()
        // 示例：保存到静态字段，或交给管理 VpnService 生命周期的组件
        return START_STICKY;
    }
}
```

## 日志收集

`collectLogs()` 返回一个 `Reader`，内容为 `application.log` 的全部文本。使用约束：

- 返回的 `Reader` 持有原生层日志文件锁，**必须立即把内容完整读出并 `close()`**；否则除 `parseRoomCode` 以外的所有 API 都会被阻塞。
- 即便忘记显式 `close()`，GC 触发 `finalize()` 时也会自动释放锁，但不应依赖这一点。

```java
try (Reader reader = TerracottaAndroidAPI.collectLogs()) {
    StringBuilder sb = new StringBuilder();
    char[] buf = new char[8192];
    int n;
    while ((n = reader.read(buf)) != -1) {
        sb.append(buf, 0, n);
    }
    String logs = sb.toString();
    // 上传或展示 logs
}
```

## 异常处理

- 原生层出现不可恢复错误（内部 panic 等）时，正在进行的 JNI 调用会抛出 `RuntimeException`。
- 房间生命周期内的可恢复错误（连接失败、EasyTier 崩溃、房主 MC 失联等）不会抛异常，而是通过 `getState()` 返回 `exception` 状态上报，具体子类型见 [HTTP API 文档](http-api.md)。
- `TerracottaAndroidAPI.panic()` 是仅用于调试的入口，调用后原生层立即 panic 并抛 `RuntimeException`，请勿在生产环境使用。

## 典型接入流程

以启动器集成「创建房间」为例：

1. 应用启动时调用 `TerracottaAndroidAPI.initialize(context, callback)`，缓存返回的 `Metadata`。
2. 用户点击「创建房间」→ 在后台线程调用 `setScanning(null, playerName)`。
3. 轮询 `getState()`：`host-scanning` 提示用户打开 MC 并对局域网开放；进入 `host-ok` 后展示 `room` 邀请码与 `profiles` 成员列表。
4. 中途若原生层需要 VPN，回调被触发，启动 `VpnService` 完成 `startVpnService`。
5. 用户点击「关闭房间」→ 调用 `setWaiting()`。
6. 需要导出诊断信息时 → 调用 `collectLogs()` 读取全部日志。

「加入房间」流程对应把第 2 步改为：先 `parseRoomCode(code)` 在 UI 层做即时校验，再调用 `setGuesting(code, playerName)`，并处理 `guest-ok` 后 `url` 字段的展示。
