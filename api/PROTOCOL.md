# 通信约定（当前实现，version 1）

本仓库实现子进程与会话侧协议；本协议的宿主截图/Agent 服务须由 Neo 主项目实现。Windows 独立 GUI 另提供用户点击触发的内置 Win32/GDI 框选截图（不调用系统截图工具或剪贴板），不是 RPC，也不改变下述权限/隐藏租约约定。启动方式见[根说明](../README.md)，方法详见 [DRAWING_API](DRAWING_API.md)。

## 帧与握手

stdin/stdout 双向 UTF-8 JSON Lines；stdout 仅协议，日志/帮助写 stderr。每个 JSON 对象占一行，最多 **65536 字节（64 KiB，不含 LF/CRLF）**，不是字符数。EOF 前完整且无换行的 JSON 可接受；损坏或超长行会被消费并产生 `protocol_error`，后续行仍可解析；若当前有待完成任务，应用还会断开宿主任务连接，避免丢失完成帧后无限等待。I/O 错误结束传输。

- 请求：`{"version":1,"type":"request","id":"neo:1","method":"get_state","params":{}}`。
- 成功：`{"version":1,"type":"response","id":"neo:1","ok":true,"result":...}`。
- 失败：同一 id，`ok:false,error:{code,message,data?}`，不得同时出现 result。
- 事件：`{"version":1,"type":"event","event":"...","data":...}`。

请求/响应 id 必须以 `neo:` 或 `runtime:` 开头，后缀非空且无空白/控制字符；会话请求 id 最多 256 字节。宿主应为每次请求使用唯一 id，并用响应 id 关联，不依赖帧的相邻顺序。params 必须是对象。成功 result 即使为 null 也必须存在，且不得出现 error；失败必须有错误对象且不得出现 result。`error:null` 不是省略 error，成功或失败响应中均拒绝。

当前消息类型的已知协议字段不得重复（即使值相同或字段名使用 JSON 转义），响应 error 对象的 code/message/data 也不得重复。未知字段仍忽略；此检查不递归扩展到 params/result/事件 data 或 error.data 的任意业务对象，不能视为所有 JSON 对象都拒绝重复键。重复已知字段会使该帧被拒绝，正常版本和 ID 下报告 invalid_message；消费该行后仍可解析下一帧。

headless 和 hosted GUI 启动发送 `ready`，data 含 `app`、`methods`、`host_methods`、`events`、`headless`、`has_window`、`max_line_bytes`、`revision_scope` 及资源能力。**ready 不含文档/页面 ID**；调用 `get_state` 或读取 configure 结果取得真实 ID 和 revision。不能硬编码示例 ID。客户端以实际 ready.methods 为准，不把 host_methods 当作可向子进程调用的方法。

configure 前仅允许 configure/get_state/close。configure 的三个权限字段全部必填且拒绝未知字段。headless 无窗口；`show` 只改变期望状态，`visible` 仍是 false。hosted GUI 在 configure 前不创建原生窗口；configure 可令有效可见状态为 true，但其结果不代表原生窗口已经显示，需观察后续状态。

## 窗口、状态与事件

`desired_visible` 保留用户 show/hide 意图；`effective_visible` 还受 configured、关闭状态及隐藏租约影响；`visible` 是 GUI 最近确认的实际状态。`has_window=false` 时不把 `visible=false` 当作“已确认所有窗口隐藏”，`hidden_confirmed` 为 false。

- `window.requested`：data 为 `{request_id,visible}`，供 GUI 适配器执行。没有对外 JSON `window.ack` 方法；原生窗口确认由应用调用 Session 内部接口。
- `state_changed`：完整 get_state 结构。
- `document_changed`：同样是完整状态；文档 ID 或 revision 改变时发送。仅切换当前页通常只有 state_changed。
- `job.finished`：`{job_id,ok:true,result}` 或 `{job_id,ok:false,error:{code,message,data?}}`。**不是 `status:"cancelled"`**。
- `protocol_error`：`{code,message?}`；常见 code 为 invalid_json/invalid_message/unsupported_version/invalid_id/line_too_long/io_error。

GUI 模式 show/hide/close/suspend/resume 的成功响应等待窗口显隐确认；更晚的请求可使旧响应得到 `window_superseded`。hide 的确认必须覆盖全部所属窗口，不能只关主窗口。窗口内 egui 面板与主窗口共同隐藏；添加独立原生窗口的适配器须登记并逐一确认。

`window.suspend` 增加外部 lease，`window.resume` 只释放对应 lease，不等于 show。重复 suspend 同一 lease 幂等；未持有 lease 的 resume 报错。内部 `capture:` lease 不允许外部释放。

## 宿主异步任务（会话层已实现）

1. 调用 capture.request 或 agent.request，带真实 document_id/page_id/expected_revision 及 `user_authorized:true`，另满足 configure 权限。立即返回子项目 `{job_id,status:"pending"}`。
2. 子项目发出独立 `runtime:` 请求。截图须先确认隐藏，再发 `host.capture_region`，含 `windows_hidden_confirmed:true`；Agent 发 `host.ask_agent`。两者出站上下文字段为 **revision**，不是 expected_revision。
3. 宿主可直接以该 request id 返回最终成功/错误；或先成功返回 `{job_id:"宿主任务ID"}`，以后发 `job.finished`（兼容单个事件别名 `jobs.finished`，不是数组）。最终事件 data 使用上述 ok/result/error 格式。宿主 job_id 非空、最多 256 字节，会话内不可复用，最多登记 4096 个。
4. 宿主最终事件 job_id 对应宿主任务；可附 `request_id`，存在时必须匹配原出站请求。不经过宿主 job_id 初始响应而直接完成的事件，必须同时带子项目 job_id 和原 request_id。重复或无法关联的结果不执行写回。
5. 子项目完成时向调用方发其自己的 job.finished。写回须原文档仍存在、原页面存在、文档 revision 未改变且权限仍有效；不接受任意不关联任务的宿主写回。

headless 和 hosted GUI 入口已将入站宿主事件交给 Session::handle_event。最终 response 与先 job_id 后最终事件两条会话路径均有实现；真实宿主服务及 GUI 联调仍需验证。宿主先返回 job_id 后应发送最终事件，不要用同一 request id 的第二个 response 替代事件。

### 截图资源与取消

宿主截图最终 result 可为已导入子会话的 `{asset_ref}`，可容纳于一帧的小图 `{png_bytes:[u8...]}`，或推荐的宿主资源描述符 `{asset_ref,total_bytes,crc32}`。携带 total_bytes 或 crc32 的结果按宿主资源描述符校验，即使 asset_ref 与本地资源同名也不能替代下载或免除宿主清理。后者 asset_ref 必须是 `asset:` 加 1..128 个 ASCII 字母数字/连字符，不是路径；crc32 为完整 PNG 字节的 IEEE CRC32 无符号 u32。

收到有效描述符后，截图已结束，内部隐藏 lease 可释放；子项目依次向宿主发 `resources.read {asset_ref,offset,length}`，length ≤8192。宿主成功结果必须含匹配的 asset_ref/offset/total_bytes、非空连续 bytes、next_offset、eof（mime_type 可提供 image/png）。全部下载后核验长度/CRC/PNG，再生成新的本地 asset_ref，发 job.finished，最后向宿主 resources.release。失败/取消同样清理上传并尽力释放宿主资源。

`jobs.cancel` 传子项目 job_id；子项目返回 `{cancelled:true,job_id}` 并发错误码 cancelled 的 job.finished。已发出的宿主任务会收到另一 `runtime:` 的 jobs.cancel，请求同时含关联的 job_id 和原 request_id。**本地取消不等于宿主采集已经停止**：截图 lease 保留到宿主取消响应 `ok:true,result:{cancelled:true,job_id?}` 或可关联的最终结果确认停止。如果取消后才收到宿主 job_id，子项目会用新 id 再取消一次；旧取消响应不能释放新一轮等待。

权限撤回、文档替换、关闭、断连也终止任务；待处理任务连同取消等待最多 32 个。Session/headless 没有通用任务超时或自动重连接口，宿主须自行设置等待期限并调用 jobs.cancel。**GUI 自己发起的截图/Agent 任务等待 60 秒后会请求取消**，也提供手动取消按钮；这是 GUI 策略，不是新增 RPC、硬实时截止或宿主已经停止的证明，截图 lease 仍须等宿主停止确认。GUI EOF 会调用断连处理，但不把 EOF 当成截图进程已停止；会话保留未确认截图 lease。headless EOF 会先断连，再对 dirty 文档尝试保存恢复包后结束；GUI 断连也会尝试恢复保存。恢复目录为 LOCALAPPDATA（缺失则系统临时目录）下 NeoRuntime-drawing/recovery，文件 recovery-<id>.neoboard；路径/失败写 stderr，不扫描文件或自动重开。输出损坏仍尝试保存，磁盘失败不保证恢复成功。

## 分块、版本与示例

对象分页和对象/图片分块都要满足完整 JSON 帧 64 KiB 限制。bytes 为 u8 数组，不是 base64，8192 字节二进制编码成 JSON 后仍有额外开销。对象大于分页预算时用 objects.read 拼接 **全部 UTF-8 字节后再解析 JSON**，不可逐块按文字解码。

文档级 revision 是乐观并发条件；分页续读必须传第一次结果 revision，修改后重取上下文。资源上传/读取本身不增加 revision，图片插入对象才修改文档。

[examples.json](examples.json) 是基本请求模板；[feature-examples.json](feature-examples.json) 是 Agent/宿主任务模板。所有 `<...>` 字符串表示调用方必须替换的动态值，数值 revision 是前提示例值。基本示例假定刚创建且未改动的会话（revision=0），添加一次文本后为 1，省略的状态响应/事件仍需调用方实际消费；功能示例展示宿主取消的事件分支，不是真实服务调用记录。文件顶层是 JSON 数组，**不能把整个文件或 pretty-print 多行对象直接写进 JSONL 管道**；逐项替换并压缩为单行。示例没有伪造 ready，也不是运行记录。
