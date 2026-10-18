# Android 图片选择与权限核验

日期：2026-10-18。修改范围：`MainActivity.java` 的文件选择器回调与页面销毁处理。

最终 `MainActivity.java` SHA-256：`a909b22736487b4434f83db8b287e85a63d045163047e80c21799553386f2f83`。

本次最直接的缺陷已通过官方源码与修改前后对照确认：**普通选择器返回仅有 `ClipData` 的选图结果时，原实现会把成功选择当作取消；网页因此没有图片，也没有错误提示。** 已统一解析所有选择器的 `ClipData` 和单个 URI。用户设备实际返回的数据尚未抓取，因此不能声明此次设备现象已经真机验收。

早先 12 项临时验证中的平台 `parseResult` 替身错误地假定支持 `ClipData`。它们仅证明实际覆盖的回调控制流，不能作为平台 URI 解析有效的证据。现已纠正并新增直接抽取官方解析函数的 17 项验证，明确的结果见 `file-chooser-clipdata-before.json` 和 `file-chooser-clipdata-after.json`。

## RCA 结论

- 状态：**部分确认**。普通选择器丢失 `ClipData` 的代码缺陷及条件下的静默结果已确认并修复；缺少用户设备原始结果和 WebView 版本，现场唯一根因仍未闭环。
- 现象：用户已明确系统选图完成，但编辑框没有图片，并且没有任何提示；应用设置里也没有照片/存储权限。
- 直接原因：原普通选择器结果走 `WebChromeClient.FileChooserParams.parseResult`。该方法的 Chromium 实现只读 `Intent.getData()`，仅有 `ClipData` 时返回 `null`，同时有两者时只保留单个 data URI。
- 根因：应用只为有序照片选择器分支实现了 `ClipData` 解析，普通 picker 和文档回退分支仍依赖不支持多选结果的默认解析器，尽管网页及 picker 均明确启用多选。
- 系统性根因：普通选择器和有序选择器没有共享成功结果解析规则；第一轮受控测试又把默认解析器的能力假定得过强，遗漏了仅有 `ClipData` 的成功结果。这些均有本轮源码和测试证据；历史发布流程不作推断。
- 关键因果链：普通 picker 成功返回 `ClipData`、`getData() == null` → 默认解析器返回 `null` → 原生回调通知取消 → 文件输入没有新增文件的 `change` → `addFiles` 未进入 → 没有缩略图、也没有网页错误提示。成功修复路径为统一提取 `ClipData` 中的 URI 后交给 WebView，后续真实文件字节读取仍待设备验证。
- 另外已修复的确定缺陷：启动失败缺少系统文档回退、`SecurityException` 未处理、旧页面回调未取消、相同 request code 误收迟到结果、回调重入时旧状态未先清空。这些启动/生命周期分支不被用作此次现场现象的替代根因。

## 证据

| 判断 | 类型 | 证据 | 来源 |
| --- | --- | --- | --- |
| 网页要求图片多选，先读取所选文件字节，再生成图片数据 | 事实 | `attachmentInput.accept = "image/*"`、`multiple = true`；`readImageFile` 调用 `file.arrayBuffer()` | `assets/remote-access/attachments.js` |
| 默认结果解析不支持 `ClipData` | 事实 | 官方固定 revision 的 `StaticsAdapter` → `SharedStatics` → `AwContentsClient.parseFileChooserResult`，最终只读 `intent.getData()` | `chromium-file-chooser-sources.json` |
| 仅有 `ClipData` 的单张、多张，以及同时有 data 的多张会被丢失或截成单张 | 事实 | 使用直接抽取的官方解析函数执行现有 `MainActivity` 方法，修复前 3 项失败、修复后通过 | `file-chooser-clipdata-before.json`、`file-chooser-clipdata-after.json` |
| 选择器失败分支在回调后返回 `false` | 事实 | 修改前源码方法的受控验证失败 | `file-chooser-before.json` |
| Chromium 在返回 `false` 后仍向上传结果接收方发送取消 | 事实 | 官方 `WebViewContentsClientAdapter.showFileChooser` | `chromium-file-chooser-sources.json`，固定 revision 和 SHA-256 |
| 销毁页面后回调未取消、旧请求结果可完成新回调、回调重入可重复完成旧回调 | 事实 | 抽取原始 Java 方法执行，三项均失败；修改后均通过 | `file-chooser-before.json`、`file-chooser-after.json` |
| 原始 picker 不可用时未回退 `ACTION_OPEN_DOCUMENT`，`SecurityException` 未处理 | 事实 | 用明确的 Android API 测试替身注入两种异常，原始代码均失败 | 同上 |
| 早先回调修改通过真实 Android API 类型检查 | 事实 | JDK 17、Android 35 `android.jar`、AndroidX WebKit 1.17.1 对 9 个 Java 源文件直接编译，退出码 0；此结果早于最终统一解析修复 | `actual-sdk-javac.json`、`actual-sdk-javac.log` |

受控验证直接抽取 `MainActivity.java` 中选择器及 `destroyWebView` 方法，使用显式 Android API 测试替身运行。最终 17 项的默认结果解析直接使用固定 Chromium revision 的真实函数，避免重新假定库能力。它验证原生控制流、URI 提取和回调状态，不模拟 WebView、ContentProvider、系统授权或图片解码。编译与测试成功不能替代真机结果。

## 已排除解释

- **“设置里没有照片权限，因此 App 一定无法选图”不能成立。** Android 官方照片选择器不要求应用取得整个媒体库权限；系统文档选择器同样按用户所选 URI 提供访问权。当前 Manifest 只有 `INTERNET` 和 `ACCESS_NETWORK_STATE`，没有请求照片权限是有意行为，本次没有新增宽泛存储权限。
- **不能把 `setAllowContentAccess(false)` 直接认定为所选文件的 `Blob` 读取失败。** 已查官方 Chromium 实现：选择结果经 `NativeFileInfo` 和 `FileChooserImpl::FileSelected` 获得特定文件读取权；文件 Blob 经 `FileStreamReader::CreateForLocalFile` 读取，Android 文件层处理 `content://` URI。`allowContentAccess` 的阻断位于 URLLoader 的 `ShouldBlockURL`。这是不同路径。当前官方源码不能替代用户设备 WebView 版本的实测，因此保留原设置，不把它列为已确认的用户故障原因。
- 有序照片选择器的可用性条件在受控矩阵中符合当前接口门槛：Android 35，或 Android 30 以上且 R SDK extension 至少为 12；不满足时继续原有文件选择路径。未发现足以支持修改该条件的证据。
- **没有继续增加解码回退。** `readImageFile` 的空文件、大小、格式和解码失败均进入错误收集并调用 `notice`，并非与用户“无提示”同等的已证实机制。`notice` 已绑定且页面有可见状态样式。没有设备 Promise 状态，不能把 `arrayBuffer` 或 `createImageBitmap` 悬挂写成确定原因。
- 网页代理另行复现并修复了“旧失败发送回执恢复覆盖正在读取的新图片”的条件竞态。它与本次 `ClipData` 丢失是两条不同链路；用户是否存在旧 pending 发送未知，本文件未修改 `app.js` 或 `attachments.js`。

## 异常状态溯源

- 适用约束：在当前文件选择请求返回 `RESULT_OK`，且 Intent 的 `ClipData` 包含所选 URI 时，必须保留这些 URI 的顺序并交给当前网页文件选择回调，不能作为取消处理。
- 依据：当前网页明确 `multiple = true`；原生 picker 使用 `EXTRA_ALLOW_MULTIPLE`。Android `ACTION_GET_CONTENT`/`EXTRA_ALLOW_MULTIPLE` 合同允许多选结果存放在 `ClipData`。
- 场景命中：仅有 `ClipData` 的单张、多张，以及同时有 data 的多张三项受控输入均命中；用户设备原始 Intent 尚未抓取。
- 值判断：仅有 `ClipData` 的成功结果被默认解析为 `null`，违反上述成功结果约束。
- 值的实际来源：系统 picker 的 Intent → `onActivityResult` 普通分支 → WebView `parseResult` → `AwContentsClient.parseFileChooserResult` 忽略 `ClipData`、只读 `getData()` → `ValueCallback(null)`。
- 最后满足约束的状态 → 首次违反点：结果 Intent 仍含有合法所选 URI → 选用默认解析器时丢失 URI，变成 `null`。
- 调查边界：固定 Chromium revision、应用实际 Java 方法与受控输入。用户设备的实际 Intent、系统选择器及 WebView 版本未知。

## 修复与防复发

- 所有选择器统一使用现有解析逻辑：先读取成功结果的 `ClipData`，再回退单个 `getData()`；移除无用的 `orderedImagePickerPending` 标志。
- 统一 `completeFileChooser`：先清空回调和请求状态，再调用外部回调，避免重入覆盖或重复调用。
- 每次文件选择分配独立的 16 位 request code，忽略迟到的旧结果。
- `destroyWebView` 先取消待完成回调并结束对应的外部选择器；Activity 销毁通过同一路径清理。
- 有序照片选择器或 WebView 创建的默认选择器无法启动时，回退 `ACTION_OPEN_DOCUMENT`，保留图片 MIME 和多选。
- 捕获明确的启动失败 `ActivityNotFoundException` 与 `SecurityException`；全部不可用时完成取消并返回 `true`。
- 未修改 Manifest、`setAllowContentAccess`、登录 UI、资源缓存及网页附件模块。
- 临时测试仅用于本次受控验证，不新增永久测试文件。最后一次清理命令被中断且没有返回输出；主代理已接管临时目录清理，本子代理遵照后续指令不再删除文件。

## 验证结果

| 验证 | 修改前 | 修改后 |
| --- | --- | --- |
| 早先 12 项选择器/生命周期验证（平台解析替身有已声明局限） | 6 通过、6 失败，验证退出码 1 | 12 通过，验证退出码 0；只用于相应控制流证据 |
| 最终 17 项验证（直接使用官方默认解析函数） | 回调修复后、统一解析前：14 通过、3 失败，验证退出码 1 | 17 通过，验证退出码 0 |
| 最终 17 项 JDK 17 harness 编译/运行 | 编译 0、运行 0 | 编译 0、运行 0 |
| 真实 Android 35 SDK 直接 `javac` 编译 | 未执行 | 早先回调修改退出码 0；最终统一解析修改由主代理重新构建 |
| `git diff --check -- MainActivity.java` | 未执行 | 退出码 0 |

直接 `javac` 的最小 classpath 未包含 AndroidX 注解元数据，出现 6 条 `RestrictTo.Scope` 元数据警告，以及已有 API 的废弃提示；没有编译错误。完整 Gradle/lint/安装包构建由主代理统一验证，本子任务没有运行。

## 未闭环项与下一步

- 缺少用户设备 Android 与 WebView 版本、原始结果 Intent。用户已确认选图完成后无图片且无提示；仍需确认现场是否命中仅有 `ClipData` 的返回形式。
- 尚未实测：点击添加图片、系统选择器打开、多选返回、真实 `content://` 字节读取、缩略图显示、取消后再次选择、图片发送、退出页面后迟到结果。
- 权限预期：选择已有图片应展示系统照片/文件选择器；不应为了该行为先弹出整个媒体库或存储权限申请。设置里不出现此类授权项符合当前最小权限设计。

## 官方合同入口

以下均为官方资料；Chromium 源码固定 revision、文件路径、行号和 SHA-256 见 JSON。

- `https://developer.android.com/reference/android/webkit/WebChromeClient#onShowFileChooser(android.webkit.WebView,android.webkit.ValueCallback%3Candroid.net.Uri[]%3E,android.webkit.WebChromeClient.FileChooserParams)`
- `https://developer.android.com/training/data-storage/shared/photopicker`
- `https://developer.android.com/training/data-storage/shared/documents-files`
- `https://developer.android.com/reference/android/provider/MediaStore#EXTRA_PICK_IMAGES_IN_ORDER`
- `https://developer.android.com/reference/android/webkit/WebSettings#setAllowContentAccess(boolean)`
- `https://developer.android.com/reference/android/content/Intent#EXTRA_ALLOW_MULTIPLE`
