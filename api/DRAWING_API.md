# Drawing / Blackboard API（当前实现）

依据 `board-session`、`board-core` 与应用入口；wire 格式、生命周期与异步事件详见 [PROTOCOL](PROTOCOL.md)。两个 app 共享会话方法，区别主要在 GUI。只在有窗口会话的 ready 中声明 capture.request；其余调用也必须先检查实际 ready.methods。库功能、GUI 控件不自动等于协议方法，本文没有 document.export、hwr.recognize 或窗口确认 RPC。

## 通用约定

下表 `C` 表示必填 `{document_id:string,page_id:string}`；`R` 表示 **C 加 `expected_revision:u64`**，revision 属于整个文档。`S` 表示完整 get_state 结果。标 `?` 为可省略字段；未标的均必填。数值须满足各自类型、范围，几何值须有限。错误响应统一 `ok:false,error:{code,message,data?}`。

所有方法可能返回 invalid_params；configure 前除 configure/get_state/close 外返回 not_configured，关闭中/已关闭时除 get_state 外返回 session_closing/session_closed，未知方法 method_not_found。完整响应超过 64 KiB（列表方法还受 max_bytes 帧预算约束）则 response_too_large。下表列附加的主要错误，不重复这些公共错误。

C 校验错误：document_mismatch、page_not_found；R 另有 revision_conflict。核心对象校验、重复 ID、页数/最后一页限制、历史状态错误等统一映射 invalid_document，不保证每种核心错误有独立 wire code。错误 message 供人阅读，不作为程序分支条件。

### S：状态结果

`app,document_id,page_id,revision,revision_scope:"document",dirty,configured,closed,connected,close_pending,desired_visible,effective_visible,visible,has_window,window_status,hidden_confirmed,hide_lease_count,permissions,owned_window_count,can_undo,can_redo,pending_jobs,pending_capture_cancellations`。

`window_status` 为 no_window/pending/visible/hidden。permissions 含课堂安全、桌面采集、Agent 三个布尔值。ready 没有文档上下文，必须从 S 取得 ID。dirty 依据保存点内容而非 revision；切页不增 revision、不变 dirty，撤销/重做会递增 revision，撤销回保存内容则 dirty=false。保存只更新保存点，不新增 revision。

## 生命周期与窗口

| method | params | result | 附加错误/行为 |
|---|---|---|---|
| configure | `classroom_safe:bool,desktop_capture_allowed:bool,agent_allowed:bool`，恰好三个字段 | S | 缺字段/未知字段均 invalid_params；默认 true/false/false。权限变化会中止外部上传，撤销相关任务权限 |
| get_state | `{}` | S | 配置前或关闭后仍允许 |
| show | `{}` | S | 设置 desired_visible=true；GUI 等显隐确认，headless 不产生窗口；没有 activate 语义 |
| hide | `{}` | S | 设置 desired_visible=false；GUI 全部所属窗口隐藏后响应 |
| close | `discard_unsaved?:bool` | S | dirty 时 unsaved_changes，除非显式 true；取消任务、上传，GUI 等隐藏确认；成功后进程退出 |
| window.suspend | `lease_id:string` | S | ≤128 字节，不能以 capture: 开头；最多 128 个 lease，同名幂等；不覆盖用户原显示意图 |
| window.resume | `lease_id:string` | S | lease_not_found；只能释放已有外部 lease，不能释放 capture: lease |

GUI 显隐请求被替代时旧请求得到 window_superseded。configure 的返回不等待原生窗口显示；其他显隐方法的成功响应等待 GUI 确认。课堂安全只强制禁止截图，Agent 另受 agent_allowed 和逐次授权限制；权限布尔值不是文件系统沙箱或进程认证机制。

## 文档、页面与对象

| method | params | result | 附加错误/行为 |
|---|---|---|---|
| document.new | `discard_unsaved?:bool` | S | unsaved_changes；新文档一页、revision=0，历史和资源重置，旧任务终止 |
| document.open | `path:string,discard_unsaved?:bool` | S | unsaved_changes、io_error、invalid_document、unsupported_version、invalid_resource、resource_not_found、invalid_png、resource_limit；先完整校验再替换 |
| document.save | `path:string` | S | io_error、invalid_document、resource_not_found、resource_limit；写同目录临时文件、同步后 rename，不预先删除旧目标，失败保持 dirty |
| pages.list | `document_id` 加分页参数 | P，数组键 pages，元素 `{page_id,object_count}` | document_mismatch、revision_conflict、object_too_large；无 page_id 时结果 page_id=null |
| pages.add | R | `{page_id,state:S}` | C/R 错误、invalid_document；新增并选择新页，最多 500 页 |
| pages.delete | R（page_id 为待删页） | S | C/R 错误、invalid_document；不能删除最后一页 |
| pages.select | C | S | C 错误；无 revision 条件，不产生撤销项 |
| objects.list | C 加分页参数 | P，数组键 objects，元素为完整 BoardObject | C 错误、revision_conflict、object_too_large |
| objects.read | R 加 `object_id:string,offset:u64,length:u64` | `{document_id,page_id,object_id,revision,encoding:"utf8_json_u8_array",offset,total_bytes,bytes:[u8],next_offset,eof}` | C/R 错误、object_not_found；length 1..8192，offset≤total_bytes |
| objects.apply | R 加 `operations:Operation[]` | S | C/R 错误、invalid_document、resource_not_found；全部原子校验、一个撤销项，失败不部分修改 |
| undo / redo | R | `{changed:bool,state:S}` | C/R 错误、invalid_document；作用于整个文档历史而非仅指定页，无可用历史时 changed=false |
| math.calculate | `expression:string` | `{result:string}` | math_error；是格式化文字，不是数值/解集 JSON |

### 分页 P

可选 `offset` 默认 0；`limit` 默认 100、上限裁至 1000、必须 >0；`max_bytes` 默认 61440，范围 1024..65536，预算包括完整响应帧。`offset>0` 时必须提供第一次返回的 `expected_revision`；首屏也可主动提供。offset 不得超过总数。

结果 P：`{document_id,page_id,revision,offset,total,next_offset,objects或pages或connections}`；next_offset=null 表示结束。objects.list 单个对象超过预算返回 object_too_large，error.data 含 document_id/page_id/revision/object_id/offset/total_bytes/next_offset。保存该 object_id，用 objects.read 在同一 revision 下分块重建对象，再按 error.data.next_offset 续读列表；版本变更后重新开始。pages.list 单项超预算仍使用 object_too_large，应提高 max_bytes，不可把页面 ID 当作对象读取。

connections.list 单个连接超过预算返回 connection_too_large，error.data 使用 connection_id（其余上下文/偏移字段同上）。应提高 max_bytes 重试；没有 connections.read，objects.read 也不能读取连接。若直接按 error.data.next_offset 续读，会跳过该连接而非取得其内容。上述列表的错误帧本身若也超出预算（例如 ID 过长），会改为不含这些定位数据的 response_too_large，不能假定错误总带 error.data。

### BoardObject 与操作

BoardObject 为 `{"id":"调用方唯一对象ID","kind":{"type":"text",...}}`，不是把 type 放在对象顶层。所有对象 ID 在文档内唯一。Operation 三种：`{op:"add",object}`、`{op:"update",object}`（完整替换，不是 patch）、`{op:"delete",id}`。

共用结构：Point=`{x,y}`，Color=`{r,g,b,a}`（u8），Style=`{color,width,dashed}`（width 0.1..100），StrokePoint=`{x,y,time,pressure}`（time 为非负秒，pressure 0..1）。

| kind.type | 其余必需字段 |
|---|---|
| stroke | `points:StrokePoint[]` 非空，`style:Style` |
| shape | `shape:string,points:Point[],style:Style`；至少两个点；bbox 或显式顶点按形状解释 |
| text | `position:Point,text:string,size:number,color:Color`；size>0 |
| math | `position:Point,layout:MathLayout,size:number,color:Color`；size>0，layout 结构见下文 |
| image | `position:Point,width:number,height:number,asset_ref:string`；尺寸>0，asset_ref 必须已在当前会话导入 |
| coordinate_system | `origin:Point,scale:number`；scale>0 |
| function_plot | `position:Point,width,height,expressions:string[],x_min,x_max,y_min,y_max`；尺寸>0，表达式非空，范围严格递增 |

`MathLayout` 是核心独立持久化结构，采用相邻标记 `{"type":...,"value":...}`（不是 TeX，也不是把节点子字段平铺）：

| layout.type | value |
|---|---|
| text | 普通字符串，不解析公式 |
| row | `MathLayout[]`，横向排列 |
| fraction | `[分子MathLayout,分母MathLayout]`，恰好两项，真正上下排版与分数横线 |
| radical | 单个 `MathLayout` 对象，根号及顶横线 |

例如一个独立的 `5/6` 结果对象（ID 仍须文档内唯一）：

```json
{"id":"fraction-result","kind":{"type":"math","position":{"x":80,"y":80},"layout":{"type":"fraction","value":[{"type":"text","value":"5"},{"type":"text","value":"6"}]},"size":26,"color":{"r":255,"g":255,"b":255,"a":255}}}
```

布局深度≤32、节点数≤512、全部 text 叶合计≤4096 UTF-8 字节；渲染另有几何/字号/字符校验，不保证任意字体可显示。`ObjectKind::Math` 作为一个对象参与列表/原子编辑、选择移动、整体擦除、撤销重做及保存/导出；普通 `kind.type:"text"` 不因含 `/`、`sqrt(...)` 或 TeX 而自动排版。

shape 可取 line/rectangle/square/triangle/right_triangle/equilateral_triangle/parallelogram/rhombus/ellipse/circle/cube/cuboid/cylinder/cone/sphere。后五种只是二维投影线框。对象入库通过结构校验不代表所有函数表达式都能成功采样。

核心另有限制：全篇最多 100000 对象、1000000 点、100000 连接、估算内容内存 32 MiB；单次 operations 最多 10000。历史内存有界（每个快照栈最多 100 项且按 64 MiB 预算裁剪），不是无限撤销；历史不写入文件，重开后不恢复历史。

GUI 选择工具为图片（含截图）和函数图提供四角尺寸手柄：固定对角，图片等比缩放、函数图宽高独立调整；尺寸下限32逻辑像素，拒绝非有限值及超编辑坐标预算。仅修改 position/width/height，不修改图片资源、函数表达式或坐标范围；实时预览不写文档，松手一次撤销/重做，过期或静止手势不提交，缩放重叠不触发函数图合并。选中图片时不显示占用角手柄的询问按钮，图片内部单击仍打开授权面板；拖动手柄不会询问Agent。函数图原有滚轮坐标范围缩放保留，不是新增RPC或schema。

### function_plot：显函数与有限隐式曲线

原 `FunctionPlot` / `kind.type:"function_plot"` schema 不变，`expressions:string[]` 中显函数仍存右端表达式，隐式曲线存规范化后的完整等式（例如 `"y^2=x"`），不拆成单支平方根，也不新增对象类型或 RPC。保存重开沿用原字段；是否保存为文件版本2仍只由 Math 对象决定，含隐式图本身不触发版本升级。

`board_math::classify_plot` / `sample_plot` 支持原显函数和具体数值系数、总次数≤2 的 x/y 单个多项式方程。`y²=x` 绘制包含上下两侧的横向抛物线，不是单值 `y=f(x)`；其他例子为 `y-x=1`、`x²+y²=9`、`(x-1)^2/9+(y+2)^2/4=1`。形状参数必须写具体数值，不接受字母 `a`、`h` 等待定参数，不声明任意隐式、高次或参数方程绘图。全平面恒等式返回 Unsupported，空集不产生曲线片段，孤立实点以小圆点（Disk）呈现；数值病态/超预算输入可报错，有限采样不承诺几何误差界。屏幕、SVG/PNG 导出及缩略图共享曲线绘制及分支裁剪，不将不同分支误连。

GUI 候选分类 `plot_expression` 保留原 `y=f(x)` / `f(x)=...` 规则，另将含 y 的合格单个隐式方程列为曲线候选；它不是自动绘图。`x=2`、`x²=2`、`2x+3=x-1` 仍留给计算，普通等式不一律绘图；手动绘图入口允许 `x=2` 竖线。点击生成的隐式图沿用局部候选缓存、防重复、擦除/撤销后有效缓存重画及每次生成一个撤销项的规则。

隐式曲线及显隐式混合图暂不支持交点搜索，返回明确的“不支持/未执行搜索”诊断，不生成虚假交点或只算显函数后冒充整图结果；原有限显函数交点搜索保持不变。以上是应用/库能力，不扩展 `math.calculate` 的求解范围或协议参数。

### 数学计算：精确常量与原有浮点路径

`math.calculate` 调用 `board_math::calculate`，仅接受 `expression:string`，仍返回 `{result:string}` 普通格式化文字，**不是 TeX、布局树、数值或解集 JSON**。不会自动插入对象；调用方可用上述 math 对象 schema 显式提交布局。支持常量实数计算、x/y 有限多项式化简、变量 x 的一元一次/二次方程、以分号分隔的两个线性方程；三角用弧度，log 为常用对数、ln 为自然对数。输入最多4096字节、整条512词元、树深64。

常量分支优先精确有理数/平方根：整数、小数和科学计数法字面量保留精确值；支持范围内四则、整数幂、平方根提取/合并与有限有理化。例如 `1/2+1/3 -> "5/6"`、`sqrt(1)+sqrt(3) -> "1 + sqrt(3)"`、`0.1+.2 -> "3/10"`。`pi`、`e`、一般初等函数、嵌套非有理根式或其他不支持的精确运算回退为 `"≈ ..."` 数值结果，不承诺所有数学恒等式均能精确化。

精确分支采用 checked i128（不是任意精度）：整数幂绝对值≤128、最多32个根式/有理项、10000次运算预算、20000次因子试除预算。定义域错误、溢出和超预算直接返回 math_error，不静默近似；只有不支持的精确运算才尝试数值 fallback，数值求值失败仍报错。多项式/方程及下列符号命令保持原 f64 算法、总次数≤12，并非所有方程已精确化，不是完整 CAS。

库公开 `calculate_display_with_bounds(input: &str, bounds: Bounds2D, options: NumericOptions) -> Result<DisplayCalculation>`，结果为 `{text:String,display:Option<MathDisplay>,approximate:bool}`；MathDisplay 包含 Text/Row/Fraction/Radical。GUI 转为核心 MathLayout：`5/6` 用上下分数、`1 + sqrt(3)` 用横排与带顶横线的根号，常量近似回退用带 `≈` 的文字叶。方程/变量多项式/符号命令沿用旧输出（display=None），GUI 上板为 ObjectKind::Text；这些旧路径的 approximate=false **不表示结果精确**，该标记仅用于常量近似回退。此库入口不是新增 RPC。

该 expression 还支持以下已实现的顶层命令（不是新增 RPC，不可嵌套或当作算术子表达式）：

| expression 示例 | 行为 |
|---|---|
| `simplify((x+1)^2)` | 展开并合并有限多项式 |
| `simplify(x+1=2;x+y=3)` | 等式或分号等式组逐式转为“左边 - 右边 = 0”；不求解、不除公因子、不做可能丢定义域的约分 |
| `diff(x^3*y,x)` | 多项式符号偏导；变量仅 x/y，另一变量视为常量 |
| `integrate(x^2,y)` | 多项式的一个原函数；积分常数或另一变量的函数取零，输入与结果总次数均≤12，f64 除法可能舍入 |

非多项式符号微积分、变量分母等不支持。此 RPC 没有矩形范围、微分点或积分上下限参数，不支持 GUI 的二元二次数值搜索、局部数值导数或定积分入口；有限多项式原函数不等于通用符号/广义积分。

黑板 GUI 的计算入口保留 `calculate_with_bounds`；结构化写板入口 `calculate_display_with_bounds` 遇方程时委托该旧路径：两式用英文分号分隔，每式总次数≤2，可指定 x/y 矩形范围，给出二元二次系统的数值候选；手写候选经用户点击计算也适用，不存在 GUI Automatic 自动计算路径。GUI 还有 f(x) 表达式的局部数值微分点与有限区间定积分按钮。它们是应用/库能力，不是新增协议方法；不承诺完整符号代数、通用非多项式符号/广义积分或完备数值求解，“未找到候选”不等于“无解”。

## 图片资源（子项目接收的 methods）

| method | params | result | 附加错误 |
|---|---|---|---|
| resources.import_png | `bytes:[u8]` | A | resource_limit、invalid_png；仅适合整帧容得下的小 PNG |
| resources.begin | `total_bytes:u64,crc32:u32` | `{upload_id,max_chunk_bytes:8192}` | resource_limit；为声明大小预留资源容量 |
| resources.chunk | `upload_id:string,offset:u64,bytes:[u8]` | `{next_offset}` | upload_not_found、upload_owner_mismatch；连续 offset、每块 1..8192 字节，不超声明大小，否则 invalid_params |
| resources.finish | `upload_id:string` | A | upload_not_found、upload_owner_mismatch、resource_integrity、invalid_png、resource_limit |
| resources.abort | `upload_id:string` | `{aborted:true}` | upload_not_found、upload_owner_mismatch |
| resources.read | `asset_ref:string,offset:u64,length:u64` | `{asset_ref,mime_type:"image/png",offset,total_bytes,bytes:[u8],next_offset,eof}` | resource_not_found；length 1..8192，offset≤总长，否则 invalid_params |
| resources.release | `asset_ref:string` | `{released:bool}` | resource_in_use；不存在返回 false |

A=`{asset_ref,width,height,mime_type:"image/png"}`。bytes 是原始字节的 JSON 数组，crc32 是完整 PNG 的 IEEE CRC32 数值，不是十六进制字符串。外部上传 owner 从请求 ID 前缀 neo/runtime 推导，不是完整请求 ID；同一上传的 chunk/finish/abort 必须使用相同前缀。finish 在 owner 验证通过后，无论完整性或 PNG 校验成功与否均消费上传，失败后应重新 begin。

单张 PNG ≤8 MiB，资源与上传预留总量≤32 MiB，资源加上传≤256 项，并发上传≤16；宽高分别≤8192，RGBA 估算及解码缓冲≤32 MiB。只支持静态 PNG，校验签名、各块 CRC、IEND、无尾随内容和完整解码；APNG 被拒绝。CRC 是完整性检查，不是身份认证。

已被文档或待执行 Agent 引用的图片不可释放；存在任意 undo/redo 历史时也保守拒绝 release。导入/上传不插入图片对象；须自行 objects.apply。resources.read/release 不接受文件路径。权限变更中止外部上传；关闭/断连中止全部上传，换文档重置资源。

## 宿主任务

| method（Neo → 子项目） | params | result | 附加错误 |
|---|---|---|---|
| capture.request | R 加 `user_authorized:true` | `{job_id,status:"pending"}` | C/R 错误、host_disconnected、authorization_required、job_limit、permission_denied、window_unavailable、line_too_long |
| agent.request | R 加 `user_authorized:true,prompt:string,asset_refs?:string[],write_back?:bool` | `{job_id,status:"pending"}` | 同上（无 window_unavailable），另 resource_not_found；asset_refs 默认 []，write_back 默认 false |
| jobs.cancel | `job_id:string`（子项目 ID） | `{cancelled:true,job_id}` | job_not_found；另发 ok=false 的 job.finished，截图仍可能等待宿主停止确认 |

capture.request 要求 has_window、classroom_safe=false、desktop_capture_allowed=true；agent.request 要求 agent_allowed=true。每次必须显式 user_authorized=true；资源引用必须存在。Agent 不自动添加当前页或所有图片，只有明确提供的 asset_refs 被传出。会话权限不能证明用户真实点击，可信宿主必须落实 UI 授权。

| 出站 method（子项目 → Neo） | params | 宿主 result / error |
|---|---|---|
| host.capture_region | `document_id,page_id,revision,user_authorized:true,job_id,windows_hidden_confirmed:true` | 最终 `{asset_ref,total_bytes,crc32}`（推荐）或本地已有 `{asset_ref}` 或 `{png_bytes}`；也可先 `{job_id:宿主ID}` 再最终事件；失败标准 error |
| host.ask_agent | `document_id,page_id,revision,user_authorized:true,job_id,prompt,asset_refs,write_back` | 最终 `{answer:string,operations?:Operation[]}` 或先 `{job_id:宿主ID}`；失败标准 error。operations 需原请求 write_back=true |
| resources.read | `asset_ref,offset,length` | 与子项目 resources.read 的进度字段相同；必须匹配描述符的上下文，详见协议 |
| resources.release | `asset_ref` | 建议 `{released:bool}`；会话不依赖该响应继续执行 |
| jobs.cancel | `job_id,request_id`（原出站请求 ID） | 确认已停止才回 `{cancelled:true,job_id?}`；失败标准 error，不能谎报停止 |

宿主真正的服务实现不在本仓库。宿主服务错误由标准 error 传播；本地完成校验还可产生 revision_conflict、permission_denied、invalid_host_response、invalid_resource、resource_integrity、invalid_png、resource_limit、response_too_large。取消/终止事件错误码包括 cancelled/permission_revoked/document_replaced/session_closed/host_disconnected/window_set_changed。成功截图 job.finished.result 为 A；成功 Agent 为 `{answer,revision}`，不会原样转发任意宿主字段。Agent operations 原子应用到请求快照页，文档换页但原页仍存在不等于版本冲突；文档改动才会使旧 revision 失效。

入站宿主 `job.finished` / `jobs.finished` 的 **ok/result/error** 格式及进程入口处理见协议；当前 headless/hosted GUI 均已转交会话处理器，但不等于已完成真实 Neo 服务联调。

GUI 图片右上询问按钮绑定单图，逐次授权后仍通过现有 agent.request；工具条 Agent 可另行授权当前页图片。授权面板新增独立的“允许本次回答写入板书”勾选项，默认 `write_back=false`，仅本次显式勾选才发送 true；构造请求后清除授权，关闭授权面板也清除。图片发送授权不等于写回授权。写回仍须宿主返回 operations 并通过原页面、文档 revision、权限及原子校验；不会把普通 answer 自动转换为板书，真实 Neo 服务联调仍未完成。GUI 自己发起的宿主任务提供手动取消和 60 秒后请求取消，不改变 jobs.cancel 的确认/隐藏租约语义；Session/headless 不提供通用任务超时或重连方法。

## 应用/库功能与协议边界

Windows 独立 GUI 的“截图（本次授权）”点击即本次本地授权：默认确认所属窗口隐藏后启动应用自有 Win32 框选层；可取消勾选“截图时隐藏窗口”以保留窗口及笔迹（可能被截入）。选项仅本次运行有效，任务开始后固定；不隐藏模式不取得/释放隐藏租约，不伪造 hidden_confirmed。拖动选区后销毁框选层、等待合成完成，再用 GDI BitBlt 读取选区像素并编码内存 PNG；不是冻结桌面快照，不依赖系统截图工具，不读写剪贴板或自动上传。仅在原文档/页面/revision 有效时自动插入，一次撤销。宽高≤8192、RGBA≤32 MiB、PNG≤8 MiB，受保护内容/HDR受GDI限制。此路径不是 capture.request RPC，不修改Session权限；hosted模式仍通过原有宿主请求及权限检查。

本地流程单槽、60秒协作式软预算；Esc、右键、失焦或显示器布局变化中止框选。成功、失败、取消、超时均等待自有截图线程退出、窗口和资源清理后，才释放本次隐藏租约，保留其他租约及show/hide意图；不再弹外部截图工具恢复确认框，不因取消保存/退出应用。系统调用不可硬中断，原生框选、混合DPI及多屏视觉效果尚待人工验收。这一清理确认仅适用于自有本地线程，不放宽宿主任务取消确认约定。

本地及 GUI 宿主截图成功上板后自动切换选择工具并选中新图片，下一次单击图片即可打开单图 Agent 面板，拖动仍移动；图片右上按钮也保留。点击只打开面板，不自动发送或授权写回，独立模式无 Agent 服务。RPC/schema 与文件版本不变。

共享 GUI 底部工具条保持单行，在原有自适应尺寸上整体放大约 10%（1920×1080 逻辑视口下高约 62、按钮约 46×46、图标约 29×29 逻辑像素），窄窗口仍优先适配。仅工具栏画笔/橡皮首次点击切换工具，再次点击打开设置，无快速双击时限；其他形状、文件、设置、更多、管理菜单及页码预览单击打开。点击别处或 Esc 取消笔/橡皮首次点击状态；菜单打开时点击原按钮关闭，菜单内部选项保持单击操作。橡皮菜单新增“全部擦除”，清空当前页全部对象及连接，保留其他页和页数，一次撤销/重做；空页不产生历史。这是 GUI 文档事务，不新增 RPC。大屏直接展示主要分组并保留“更多”，窄屏把形状、画笔属性、页面、文件、设置、撤销/重做等收入“更多”；工具条二级菜单向上展开并限制高度、支持滚动。黑板收起仍将同一原生窗口缩为 220×64 的展开/退出入口，画板收起仅折叠工具栏、保留笔迹、不缩小或隐藏窗口；文档与撤销历史不变，收起不是全部窗口隐藏确认。这些 UI 变化没有新增协议方法。

GUI “将手动输入计算后写到板上”经 `calculate_display_with_bounds` 只新增一个结果对象，原输入留在面板，不拼成长串原式加结果；常量结构为 Math，旧方程/符号输出为 Text。手写候选由用户点击计算后同样使用此结构化结果，保留原笔迹、另补一个结果对象。Math 的选择范围、整体移动/擦除与撤销重做使用真实二维布局；PNG/SVG 与屏幕共享布局，保留上下分数、分数横线、根号及顶横线，仍受导出字体/资源限制，不等于已做原生 GUI 视觉验收。

`board-hwr` 模板及 GUI 神经候选适配仍返回 `requires_confirmation=true`。黑板 GUI 默认 Off，显式“启用手写识别（点击图标才计算/绘图）”（内部 Confirm）仅本次运行有效；已移除 Automatic 及未校准模型自动计算授权。停笔 2.5 秒只触发后台识别，不自动弹数学面板，不启动计算或写回，也不是完成截止。保留默认模板与个人模板 JSON 学习/保存/加载；模板相似度、神经 logprob 均仅供核对，不作为自动计算门槛。

可选 TexTeller 后端须用户明确绝对目录并点击后台加载/重载，不自动下载；模型需单独准备，固定版本、哈希、官方 Apache-2.0 来源和安装命令见[模型说明](MODELS.md)。Rust/ONNX Runtime CPU 推理，当前 Windows runtime 静态链接，无 Python 推理进程。原始 LaTeX、EOS、词元数及平均生成词元 log-softmax 可查看；未知命令、歧义或不支持结构完整拒绝，不静默删未知部分计算。神经 confidence 为0，不把 logprob 当正确概率；高分也须由用户点击才计算。

识别后原笔迹右上角的计算器图标点击计算算式/方程，曲线坐标图标点击生成函数图像（无需先转成文字），纠错图标在识别/LaTeX 转换失败时仅打开原输入及纠错，不计算；不再使用 `=` / `f` / `?` 文字按钮。可在数学面板修改后点击“确认纠错并计算写回 / 生成函数图像”。识别、数学或模型加载忙碌时图标禁用，不能重复提交；计算器图标可提示“计算中”。成功在算式右侧/方程下方补一个结果，或添加函数图；一次撤销移除新增对象并保留原笔迹。

多个候选仅在 GUI 内存按 document_id/page_id 缓存，不持久化、不新增 RPC。来源笔迹与邻域对象均按完整内容快照校验；邻域是原笔迹包围框 `expand2(100, 28)`，按逻辑坐标横向/纵向外扩及对象包围框相交判定，不是严格逐像素周围。远处修改、切工具、切页再回来保留有效候选；来源改动/删除或邻域对象增删、移动、内容改变使相关候选失效。自身结果对象 ID 排除于邻域校验，结果存在时隐藏图标、防止重复写入；只误擦/删除或撤销自己的结果且原式/邻域仍有效时可再点击生成，沿用结果 ID，重做恢复后仍防重复。每文档估算缓存预算 64 MiB、最多 4096 项，每页最多 1024 项；超额拒绝新候选，不淘汰已有有效项，不是无限缓存。成功 document.new/open 清空缓存并更新生命周期，包括打开相同文档 ID 的文件。

上述局部缓存不放宽后台写回校验：后台仍严格匹配文档/页面/revision；远处修改使 revision 变化也可取消本次任务、丢弃旧结果，但保留局部快照仍有效的已缓存候选，工作槽排空后可点击重试。继续书写、隐藏、取消/关闭、切模式/后端、改目录/重载会取消当前任务/活动上下文，不等于清空全部有效缓存；Off 或隐藏时不显示图标。

手动数学面板计算、化简、解方程、数值微分/定积分、绘图和写板按钮仍由点击执行。**禁止 GUI 自动计算不等于禁用 `math.calculate`**：该方法本来就是调用方主动发起的 RPC，仅返回文字、不自动插入对象，接口、宿主权限和 JSON Lines 协议均未改变。上述手写交互属于黑板 GUI/库能力，没有新增 hwr.recognize、模板训练、模式配置或后台计算 RPC，也不表示透明画板新增数学面板。

HWR、数学、交点搜索各自单槽后台执行、无排队，取消是结果失效而非强杀线程；旧工作排空前不启动同类任务。GUI 模板预算仍为32笔/8192点，TexTeller 为128笔/32768点；神经最多256生成词元、60秒软预算，只在 ORT 调用之间检查，不 kill 调用、不保证完成截止。模拟候选或合成指针测试不等于真实模型/原生视觉验收。模型测试需显式 `TEXTELLER_MODEL_DIR` / `--ignored`；构建方式见[根说明](../README.md#build-on-windows)，验收要求见[发布清单](../RELEASING.md)。模型识别和有限数学能力不等于通用数学问题已全部解决。

共享 GUI 橡皮按下/拖动实时预览，预览不改文档/revision/历史；有效松手一次提交、整段手势一个 undo/redo，未命中不新增历史，取消/上下文失效丢弃预览。这不是新增擦除 RPC。

GUI 使用 `board-render` 的资源导出接口：PNG 合成图片和字体文字；SVG 内嵌 PNG，注入字体时文字转字形路径，无字体时保留依赖查看器字体的 text。缺图片、PNG 文字无字体或注入字体缺字会报错。GUI 尝试固定 Windows 中文字体路径；渲染库不自行扫描字体/图片路径，资源引用只是键。不存在 document.export 或资源 SVG 上传 RPC，资源传输仍仅 PNG。

## 保存格式

无图片时使用核心文件：`{version:V,document:{id,pages,current_page,revision,connections}}`，pages 为 `{id,objects}`，current_page 为0起算索引。历史/权限/窗口/任务不在文件中。旧文件未写 connections 时按空列表加载。

有图片时保存：`{format:"board-session-package",version:V,document:{...},resources:[{asset_ref,png_hex}]}`；document 是原始 Document，不再嵌一层核心 version。png_hex 为内嵌 PNG 十六进制字符串，只保存当前文档引用资源。

两种格式均按**当前文档是否含 Math 对象**选择最小版本 V：含 Math 为2，只有旧对象为1；有无图片只决定是否使用 package。新程序读取版本1/2；版本1内容中出现 Math 返回 invalid_document，未知版本返回 unsupported_version。Math 布局原样持久化，重开仍为一个二维对象。旧版程序不能读取含 Math 的版本2文件/包，不能宣传为向旧程序透明兼容；JSON Lines 帧版本仍为1。

ready.data 增量能力字段（不是新增 `capabilities` 容器或数学方法）：

- `document_file_versions: [1, 2]`
- `resource_persistence_versions: ["board-session-package-v1", "board-session-package-v2"]`
- 旧 `resource_persistence: "board-session-package-v1"` 字段仍保留；不要仅凭此旧字符串判定只能读写v1，以新增版本数组和实际文档内容为准。

加载仍拒绝缺失/重复/未引用资源、未知格式与无效 PNG。Session 文件上限80 MiB，含 hex 膨胀；core 独立 JSON 上限128 MiB 不覆盖 Session 的更严格上限。打开成功重建干净保存点，撤销历史仅保留在当前内存会话；保存成功不会清空当前内存历史。

## 持久连接

以下方法已在 Session execute 和 ready 中实现，连接参与文件持久化、dirty、撤销重做和对象变更传播。

| method | params | result | 附加错误 |
|---|---|---|---|
| connections.list | C 加分页参数 | P，数组键 connections，元素为 Connection | C 错误、revision_conflict、connection_too_large/response_too_large |
| connections.connect | R 加 `connection:Connection` | S | C/R 错误、invalid_document；原子建立并将线端点传播到锚点，一个历史事务 |
| connections.disconnect | R 加 `connection_id:string` | S | C/R 错误、invalid_document（指定页无此连接）；断开不移动线端点 |

Connection=`{id,page_id,line_id,line_endpoint,target_id,target}`。line_endpoint 只能 0 或 1；target 为 `{type:"vertex",index}` 或 `{type:"edge",start,end,t}`（start/end 不同，t 在 0..1）。connection.page_id 必须与请求一致，源线与目标同页；源对象必须是双点直线，目标须是显式顶点形状（非直线至少三点，双点 bbox 先展开）。拒绝自连接、重复连接 ID、同一端点重复绑定、依赖环、锚点越界及连通分量内二维/三维混接。更新目标传播线端点，删除关联对象清理连接。GUI 已在直线/端点手势提交时建立连接，也提供断开所选直线操作；不是 3D 实体约束求解器。
