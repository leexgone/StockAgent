# 0015 · Web 对话框文件上传识别：multipart 随消息发送，robit 零改动

- **状态**：接受（承接 [0014](./0014-agent-standard-tools.md) 标准工具面与 [0011](./0011-agent-channels-and-server-side-history.md) 通道独立会话；QQ 附件链路口径见 `07` R10）
- **日期**：2026-11-14
- **决策者**：leexgone（产品所有者），Claude Code 辅助整理

## 背景

QQ 通道附件链路已随 robit 0.2.0/0.2.1 收口（落盘 `media/` → 标注行带本地路径 → 图像 vision / 非图像 read/bash），ADR-0014 又启用标准工具面（read/bash 自动执行）——子木「解析上传文件」的手已备好。但 **Web 通道今天只能发纯文本**（`ChatRequest{text}`，`submit_user_text` 注入 attachments 恒空），主人没法在网页对话框递文件。本轮补齐 Web 侧「递文件」管道，与 QQ 同链。

调查闭合的关键事实：**robit 三 crate 零改动即可支持**。`WebBridge::send_message(FrontendMessage::UserInput{text, attachments})` 是公开通道（子木 cancel 已在用）；`MediaAttachment` 全 pub 字段可宿主结构体字面量构造，`local_path` 编码优先于 url；`describe_with_local(base)` pub。Agent `build_user_message` 自动分流：支持图像 + 图像附件 → 本地优先 base64 vision；非图像/不支持 → 逐附件追加路径标注行（标准工具按路径读）。缺的全在宿主：上传 HTTP 面 + 落盘消毒 + 注入点扩 attachments。

用户拍板：① 契约形态 = **随消息发送 multipart**；② 交互 = **📎 按钮 + 拖拽 + 粘贴**全套。

## 决定

1. **multipart 随消息发送**：`POST /api/chat` 由 `Json<ChatRequest{text}>` 改 `multipart/form-data`（`text` 字段 + `files` 文件字段）。服务端收到即原子落盘 + 消毒 + 注入，与 QQ 接收语义同构——单一接收点、无孤儿文件、无需客户端回传路径引用。空 text 允许（有文件时）。前端走 `/api/chat*` 原生 fetch 例外（FormData，浏览器自设 boundary）。
2. **robit 零改动**：注入改走 `web.send_message(UserInput{text:原文, attachments})`；`MediaAttachment{local_path:Some(落盘路径), content_type, filename, size, url:""}` 宿主构造。图像识别免费获得（主模型 supports_images=true），非图像靠 ADR-0014 已启用标准工具。
3. **扁平 `media/` + 复刻消毒**：与 QQ 同根（人格 `agent.md` 已指向「media/ 下」，本轮无需改人格）。robit `sanitize_filename`/`resolve_collision` 是私有 fn，宿主复刻同等规则（file_name 分量剥目录与 `..` → 滤控制字符及 `/ \ :` → trim 空白与首尾点 → 空→UUID v4 → 按 content_type 补扩展；防覆盖 `name-1/-2…100→uuid`，绝不静默覆盖）。流式累计守卫，超限删半截 → 413。
4. **限额复用官方键**：`[app.context] max_media_bytes`（缺省 20MB，0=不限，QQ 同款），不自创键；`max_files` 宿主政策（缺省 10，文档注明）。
5. **档案标注版 text + 原文注入**（QQ 同款，零 schema 改动）：`submit_user_input` 先落档「原文 + 每附件 `describe_with_local(Some(working_dir))` 标注行」，再注入原文 + attachments——Agent 恰好标注一次不双重；复活 `load_history` 只回灌 r.text，标注随档案留存。`submit_user_text` 保留为薄包装（调度器/reset 用）。**先落档再注入纪律不变**。
6. **handler 可测结构**：lib 侧 `chat_upload.rs`（helper + handler），`UploadHook` 照 `ResetHook` 注入范式与 bin AgentBridge 解耦——上传端点得集成测试覆盖（bin 专属端点今天无覆盖）。
7. **保留策略本轮不实装**：media/ 与 QQ 共享，QQ 启用时 `rotate_media` 顶层旋转覆盖 web 文件；QQ 关闭（缺省）时无旋转 → 开发期接受累积，**R12 登记**（robit 公开 sanitize/collision/rotate_media 供宿主复用 + Web-only 保留缺口）。

### 信任边界（明示）

上传端点无鉴权（与本服务一致，监听 localhost），只向 `media/` 写消毒后文件、**上传本身不执行任何东西**；解析由 Agent 按 ADR-0014 人格纪律自动执行——属 ADR-0014 已接受扩张内，非新增风险面。文件名消毒防路径穿越；MIME 信任客户端（robit 同款），图像编码失败自动降级标注行。

## 选项与取舍

- **multipart 随消息发送（选）**：原子、QQ 同构、无孤儿、无路径引用校验、单端点。本机服务上传毫秒级，发送前 chips 用本地 File 元数据即可，无需预上传进度。
- **两步先传后引用（不选）**：`POST /api/chat/upload` 先落盘返回引用 + JSON 带引用发送——可预上传/进度，但需校验客户端回传路径不越界、处理「传了不发」孤儿、双端点双状态，复杂度换不来本机场景收益。
- **base64 JSON（不选）**：纯 JSON 契约最简，但 33% 膨胀（20MB→27MB body）、全量进内存、DefaultBodyLimit 须放宽；multipart 流式落盘常数内存更优。
- **`media/web/` 子目录（不选）**：可豁免 QQ 顶层旋转，但共享扁平目录 = 共享消毒/防覆盖/保留约定更简，人格已指向 media/ 根；跨通道旋转耦合如实记录于 R12。
- **类型白名单（不选）**：QQ 同构接受任意类型，可读性由 Agent 判（图像 vision、文本/数据 read/bash、不可读如实说）；白名单收窄能力且无安全收益（消毒已防穿越）。

## 后果

- 契约 multipart 化：`ChatUploadForm{text, files:Vec<String> /*format=Binary*/}`，utoipa 原生 `content_type="multipart/form-data"` + `#[schema(value_type=String, format=Binary)]` 表达（5.5.0 已核实）；响应 202 不变 + 413；openapi.json 再生成 + 前端 gen:api。
- 依赖：workspace axum 加 `features=["multipart"]`（现未开，锁无 multer）；tokio 显式补 `"fs"`；uuid v4；reqwest dev multipart（集成测试）。Multipart 流式提取不受 DefaultBodyLimit 2MB 约束，守卫自写。
- 前端：ChatPanel 📎/拖拽/粘贴 + chips（中性色不占 --accent 预算）+ `Entry` user 扩 files 元数据；`src/lib/fileAnnotations.ts` 纯函数解析 robit 标注行（live 与 hydrated 统一 chips，失败降级纯文本）；vitest node 环境直测，无新 dep、无 jsdom。
- 人格无需改（`agent.md:33` 已预留 media/ 上传口径）；persona 守护断言不变。
- **R12 新登记**移交 robit（公开消毒/防覆盖/旋转 API + Web-only 保留缺口）。
- 回滚路径：前端回退纯文本 FormData（仅 text 字段）+ 后端 handler 回退 Json 即恢复旧契约（git 一笔 revert）；media/ 落盘文件与 QQ 共享，回滚不影响 QQ 链路。