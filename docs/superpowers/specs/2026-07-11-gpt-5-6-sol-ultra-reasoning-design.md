# GPT-5.6 Sol Ultra 思考等级设计

## 目标

为 `gpt-5.6-sol` 增加 `ultra` 思考等级，并确保该等级仅对 Sol 型号生效。

支持以下 Sol 命名形式：

- `gpt-5.6-sol`
- 带供应商前缀的名称，例如 `openai/gpt-5.6-sol`
- 带日期或版本后缀的快照，例如 `gpt-5.6-sol-2026-07-09`

以下模型继续保持现有最高 `max` 等级：

- `gpt-5.6`
- `gpt-5.6-terra` 及其变体
- `gpt-5.6-luna` 及其变体
- 其他 GPT、Claude 和第三方模型

## 实现设计

### 模型识别

在协议代理的模型能力逻辑中增加统一的 GPT-5.6 Sol 判断。判断时先规范化大小写、去除供应商前缀，再匹配精确名称或 `gpt-5.6-sol-` 快照前缀，避免 `gpt-5.6-custom`、Terra 和 Luna 被误判。

### 思考等级能力

全局思考等级顺序扩展为：

`minimal → low → medium → high → xhigh → max → ultra`

Sol 的可选等级为：

`minimal、low、medium、high、xhigh、max、ultra`

其他 GPT-5.6 模型仍为：

`minimal、low、medium、high、xhigh、max`

`ultra` 需要进入统一的规范化、排序和能力钳制逻辑。这样 Sol 请求可以保留 `ultra`，不支持该等级的模型收到同类配置时会按现有规则降到自身最高支持等级。

### 模型目录与 UI

生成模型目录时为 Sol 输出 `ultra` 及其说明，配置中的 `model_reasoning_effort = "ultra"` 可以被正确读取。

Codex App 注入脚本的 fallback 能力表同步增加 Sol 专属 `ultra`。正常情况下 UI 优先使用后端生成的模型目录；后端目录不可用时，fallback 仍保持相同能力范围。

不修改用户当前在 `assets/inject/renderer-features.js` 中已有的 Fast 提示文案改动。

### 协议行为

Responses API 请求继续直接发送到 Responses 上游，Sol 的 `ultra` 原值透传。

Chat Completions 和 Anthropic 转换继续使用现有能力钳制逻辑。除非对应模型能力明确包含 `ultra`，否则不会向不支持的上游发送该值。

## 验证

扩展现有长期回归测试，验证：

- Sol、供应商前缀 Sol、Sol 快照均包含 `ultra`。
- `gpt-5.6`、Terra、Luna 和自定义 GPT-5.6 名称不包含 `ultra`。
- 配置解析接受 `ultra`。
- 生成的 Sol 模型目录包含 `ultra`，非 Sol 模型不包含。
- Sol Responses 请求保留 `ultra`。

完成后运行相关 Rust 测试、前端检查、格式检查和 `git diff --check`。本次不新建临时测试文件。
