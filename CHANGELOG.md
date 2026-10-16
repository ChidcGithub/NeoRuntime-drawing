# Changelog

This file records changes in English and Chinese. The v0.0.1 and v0.0.2 sections and their validation records remain historical for their respective tags. Publication and tagging are separate maintainer actions described in [RELEASING.md](RELEASING.md).

## v0.0.2

Second source-only prerelease. The headline changes: broken wgpu drivers no longer take the app down at startup (child-process backend probing with automatic fallback), pen widths are smoothed so thick strokes draw and erase cleanly, computed answers are sized and placed to match the writing they answer, and ink buttons disappear when the expression cannot be recognized. JSON Lines stays at version 1; document formats stay at v1/v2/v3; no new RPC is introduced.

### English

#### Broken wgpu drivers no longer take the app down at startup

- When the selected renderer is wgpu, the app now probes candidate backends (Vulkan → DX12 → GL) in a child process before creating any window: the app re-invokes its own executable with a hidden `--render-probe <backend>` flag, which performs offscreen instance, adapter, and device initialization and exits 0/1. Each candidate gets an 8s budget (crash, error, or hang = failure, the probe process is killed); when all candidates fail the app falls back to Glow. Because probing runs out of process, even drivers that hard-crash (access violation/abort) only take down the probe. An explicit `NEO_DRAW_DEBUG_BACKEND` override skips probing — the diagnostic path still reproduces the original crash on purpose.
- The first implementation probed on a child thread, which cannot catch driver access violations; it was replaced by the child-process version before landing. The probe does not create a window surface and captures nothing.
- New `renderer_probe` module (`candidates`, `backend_name`, `probe_gpu`, `probe_wgpu_backend_via_process`, `run_probe_cli`) with unit tests for ordering and name mapping; `--render-probe` is intercepted in `parse_args` before GUI/protocol parsing and never reaches the JSON Lines path.
- Verified: board-app suite **321 passed, 0 failed** (316 before plus 5 probe/parse tests); `cargo fmt --all --check` clean. The gui.rs wiring is applied by the reviewed script `.dbg/apply_gui_patch4.py` (the workspace write guard misflags a pre-existing `authorization` field line in that file). On a healthy machine the probe selects the platform backend and startup proceeds normally; broken-device behavior needs the classroom hardware.

- Release validation: `cargo test --workspace --locked --no-fail-fast` — **708 passed, 12 ignored, 0 failed** across 27 test binaries; `cargo fmt --all --check` clean; `cargo build --release -p neo-drawing -p neo-blackboard --locked` succeeded. Environment: Windows x64 MSVC, rustc/cargo 1.98.0. Release builds are not distribution assets.

#### Smooth stroke widths and fragment-start continuity

- Stroke widths derived from writing speed are now computed from a ±30ms time-window moving average of per-segment speeds, applied twice (equivalent to a triangular kernel with ±60ms support, center-weighted), instead of raw instantaneous samples: mouse/tablet timing jitter was being amplified by the base pen width into jagged, scalloped edges on thick pens. The time window gives the live preview (sparse pointer events) and the committed resampled stroke the same smoothing scale, only removes high-frequency jitter (not slow-fast trends), and leaves genuine pauses and deliberate speed changes unaveraged. The first point of a stroke now takes its width from the first segment's speed instead of resetting to the thickest value: erasing no longer makes surviving fragments pop back to the full base width — a fragment's start matches the writing speed at the cut, and genuine slow pen-downs still start thick.
- Both width implementations (board-ink and the render-side replica) were changed in lockstep; their equality is guarded by an existing test. New regression tests cover jitter smoothing (uniform speed with alternating event intervals now stays within 2px of adjacent width steps at width 40) and fragment-start continuity (a mid-stroke fragment keeps its local 25px width instead of resetting to 40px). The velocity-contrast and no-join-overdraw test passes unchanged: segments separated in time are not averaged.
- Verified: `cargo test --workspace --locked --no-fail-fast` — **705 passed, 12 ignored, 0 failed** across 27 test binaries (703 before this change plus two new width tests), including the full board-render suite with the million-point render test; `cargo fmt --all --check` clean. The first time-window implementation had an O(n²) worst case on long equal-timestamp plateaus (the million-point test crawled); the sliding window now maintains a running sum and is strictly O(n) — the million-point test finishes in ~1.5s.

#### Ink buttons disappear when the expression cannot be recognized

- When recognition or LaTeX conversion fails, ink candidates no longer show the correction button (and there is no calculator button to click): unrecognized writing shows no ink buttons at all. The calculator and plot icons for recognized candidates and the busy “计算中” indicator are unchanged; the math panel remains available for manual input, and the failure message now points there instead of to a correction icon.
- The `Icon::Correct` variant and its drawing were removed; the canvas loop skips unrecognized candidates instead of rendering a correction button.
- Verified after applying the reviewed patch script `.dbg/apply_gui_patch2.py`: `cargo test --workspace --locked` should report **703 passed, 12 ignored, 0 failed** (three assertions now require `ink_action().is_none()` for failed recognition; the malformed-LaTeX preview test was renamed to `malformed_latex_shows_no_ink_buttons_and_keeps_preview`). Record the actual run in this entry when applying.

#### Answers now match the writing they answer

- Ink-candidate results are sized from the source writing: the answer's character size is the 80th percentile of the source strokes' per-stroke heights (bars, dots and equals signs cannot drag it down; multi-line gestures still yield a line height), clamped to 14..400 with 26 as the empty fallback.
- Handwritten answers take the source gesture's pen width stroke by stroke (clamped 0.1..100); without a source width the learned style width is kept.
- Placement: equations go below the source, left-aligned; other results go right of the source, vertically centered on it. The gap scales with the source height (0.4×, clamped 14..48). If the answer's bounds cannot be measured the previous anchor position is kept. The fixed-size 26 and top-aligned placement are gone.
- Verified: features unit tests cover the size percentile, centering, gap, width override and degenerate bounds; the four GUI tests that locked the old fixed placement were updated to the new contract. The gui.rs edit for this change is applied by the reviewed script `.dbg/apply_gui_patch.py` because the workspace write guard misflags a pre-existing `authorization` field line in that file; run it once, then `cargo test --workspace --locked` and `cargo fmt --all`.

#### Smoother personal handwriting motion and two more learned style dimensions

- Synthesized handwriting answers now move: per-stroke timestamps use a bounded ease-in/ease-out profile whose total duration stays `path_length / speed`, so the learned speed keeps setting the pace while ends slow down and the middle speeds up. Structural fraction bars and radicals share the same motion. Manual samples keep their exact authored geometry and timing.
- Font-skeleton fallback glyphs (only those; templates and manual samples are untouched) are smoothed with a single bounded pass so pixel-thinned zigzag reaches the board less. Smoothing strength follows the learned corner sharpness. Oversized inputs beyond the 2,048-point sample budget are kept verbatim.
- Automatic style learning now estimates two more bounded scalars per stroke: a peak-to-mean speed ratio (1.0..2.2, drives the ease strength) and a corner-sharpness ratio (0..1, drives the fallback smoothing strength). The observation window stays 31 and observations still store no unlabeled points. A planned closed-loop rejection filter was dropped before implementation: handwritten `o`/`0`/`e`/`8` are loops too, and the rolling median already bounds occasional diagram strokes.
- Profile files now write format **3** and read formats **1**, **2**, and **3**; older files load with the motion dimensions at their defaults (speed peak 1.35, corner 0.5). Format 1 still cannot carry learned style. Old programs reject format 3 files, as they already reject newer document versions.
- The handwriting-preview path applies the learned corner sharpness; the math-panel answer path currently calls the two-argument fallback entry and therefore uses the default 0.5 until its call site adopts the learned value.
- Verified: `cargo test --workspace --locked` — **701 passed, 12 ignored, 0 failed** (baseline before this change: 697 passed, 12 ignored, 0 failed); `cargo fmt --all --check` clean. Two assertions that encoded the old profile format version were updated with that deliberate format change (`version == 2` → `3`; the invalid-version probe list now uses 4 instead of 3); no other test was weakened. Native GUI handwriting readability on real hardware remains pending as before.

#### Runtime-adapter compatibility preparation

- Fix hosted `close` before `configure`: the pre-native-window startup adapter completes the accepted hide/close request without waiting for another stdin frame or EOF. Configured sessions still require real window acknowledgement; dirty-content guards and capture-stop barriers are unchanged.
- Add 15 windowless startup regressions and 19 existing-protocol compatibility fixtures covering both applications, strict configuration, directional capabilities, cancellation/revocation, dirty close, EOF, resource chunks and lossless handwritten v3 persistence. JSON Lines remains v1; no Mod manifest, grant, generation field or new RPC is introduced. Official registration and process-generation fencing remain host-adapter responsibilities.
- Verified the held-open-input regression failed before the fix. After this follow-up, `cargo test --workspace --locked --quiet`: **697 passed, 12 ignored**; formatting and diff checks passed. No native GUI/capture acceptance is claimed. This is a separate follow-up to `8c32db0`, not a change to the main Neo v0.1.0 tag or its locked runtime.

#### Personal handwritten answers and fixes

- Add experimental local personal-style answers, enabled by default independently of HWR (still off by default). Newly committed local pen strokes update bounded numeric style statistics; exact-glyph samples remain optional and manually labelled. Calculation/write-back still requires a click; host Agent answers are not personalized. No neural training, model download, or new RPC is introduced.
- Fix input/result ordering: keyboard edits, Esc, and relevant pointer/settings changes are handled before already-ready background results in the same frame. Held input does not indefinitely block result draining; sampling-area Ctrl+Z takes priority over document undo while a draft is active and text input is not focused.
- Align synthesized fraction bars and operators on a shared math axis. Account for clamped pen width and bounded miter joins in spacing; preserve lowercase metrics so `o` differs in size from `0`, and use a hooked `x` distinct from `×`. These changes do not guarantee visually unambiguous handwriting.
- Move handwriting preview generation and profile JSON I/O to one shared background slot without a queue. Reject stale preview/load results after relevant context changes and reuse cached preview meshes on idle frames; previews do not modify the document or learning statistics.
- Profile save/load still requires an explicit local path and overwrite/replacement confirmation. An authorized save writes the click-time snapshot asynchronously: closing the panel or editing the profile does not interrupt an in-progress write, and later edits are not included. There is no automatic profile persistence or loading.
- Cache unstyled font skeletons and failure results per loaded font generation, shared by its snapshots: at most 128 characters, 65,536 points, and 2 MiB of accounted entry payload. Successful font replacement starts a new generation; outstanding snapshots can retain the old one. Missing/over-budget glyphs preserve the entire standard-rendered answer with a diagnostic, rather than partial personalization.

#### Straight-line intersections, rendering, and persistence

- Extend plot **求点** to straight lines written as `y=2x+1`, `f(x)=2x+1`, `y-x=1`, or manually plotted `x=2`, including coordinate-axis intersections and mixtures with supported explicit functions. Parallel lines return no pairwise point; coincident lines sharing a segment within the rectangle report non-discrete intersections; contact at only one rectangle corner returns that single point.
- Line/line intersections use analytic **f64 floating-point** formulas, not exact rational arithmetic. Near-singular systems and evaluation/residual failures can report errors. Vertical-line/explicit-function pairs evaluate at the fixed x; other explicit pairs retain bounded, incomplete numerical search, with candidates clipped to both plot axes.
- Fix boundary clipping to absorb at most four ULPs of floating-point rounding, preserving decimal boundary intersections such as `0.1*x+0.2` at `x=1`, `y_max=0.3`. Clamped points must pass machine-precision-scale residual checks; the root-finding tolerance does not enlarge the rectangle. Numerical-search candidates are also rechecked against both original expression ASTs, preserving their domain and arithmetic-error checks.
- General implicit quadratics remain unsupported for 求点. An unsupported curve among the inspected first 16 prevents the whole-plot search; the explicit subset is not presented as a complete result. Larger plots receive a limit diagnostic. This does not add an RPC or expand `math.calculate`.
- Render committed pages using document-revision caching, keeping temporary gesture previews separate. Bound retained page mesh vertex/index buffers, including aggregate batches, to 64 MiB per page renderer. This is **not a global RAM cap**: source snapshots, scenes, resources, submitted frames, other caches, and application state are separate. Bound GUI font-file reads to 64 MiB and validate font data before installation.
- Read document/package versions v1/v2/v3 and save the smallest required version: `Handwritten` → v3; otherwise `Math` → v2; otherwise v1. Images only determine resource packaging. Frozen whole-answer strokes, standard text, and optional math layout survive reopening without a profile; older v1/v2 readers cannot read v3. Profile JSON versions are separate; JSON Lines remains version 1.

#### Validation record and remaining acceptance

- **Final verification record (reported by the maintainer):** `cargo test --workspace --locked --quiet` completed with **663 passed, 12 ignored, 0 failed**; `cargo fmt --all --check` and `git diff --check` passed. Release builds of both `neo-drawing` and `neo-blackboard` succeeded in **2m 23s**. Ignored tests are not counted as passed. The v0.0.1 test/build record below remains historical; these results were not rerun for this documentation-only update.
- Native GUI checks remain pending for keyboard/cancellation ordering, preview/profile workflows, handwritten math and complex Chinese readability, and straight-line 求点. Target i5/3 GB responsiveness and whole-machine memory measurements remain pending; bounded caches do not establish target-machine acceptance.
- No release, tag, publication, or new license/provenance clearance is asserted here.

### 中文

#### 坏 wgpu 驱动不再让应用启动即崩

- 选定渲染器为 wgpu 时，应用现在在创建任何窗口之前，以子进程探测候选后端（Vulkan → DX12 → GL）：应用以隐藏参数 `--render-probe <backend>` 重新拉起自身，做离屏实例 + 适配器 + 设备初始化后以退出码结束（0 成功/1 失败）。每个候选 8 秒预算，崩溃、报错、挂起都算失败并 kill 探测进程；全部失败时回退 Glow。因为探测在进程外，驱动内的硬崩溃（访问冲突/abort）也只会带走探测子进程。显式设置 `NEO_DRAW_DEBUG_BACKEND` 时跳过探测——诊断路径本来就要复现原崩溃。
- 第一版探测在子线程上，无法捕获驱动访问冲突；落地前已替换为子进程版本。探测不创建窗口表面，不采集任何内容。
- 新增 `renderer_probe` 模块（`candidates`/`backend_name`/`probe_gpu`/`probe_wgpu_backend_via_process`/`run_probe_cli`）与排序、名字映射单元测试；`--render-probe` 在 `parse_args` 中先于 GUI/协议解析拦截，绝不进入 JSON Lines 路径。
- 验证：board-app 套件 **321 passed、0 failed**（原 316 加 5 个探测/解析测试）；`cargo fmt --all --check` 干净。gui.rs 的接线由审查后的脚本 `.dbg/apply_gui_patch4.py` 应用（工作区写守卫误报该文件既有 `authorization` 字段行）。健康机器上探测选定平台默认后端、启动如常；坏设备的实际表现待教室硬件验证。

- 发布验证：`cargo test --workspace --locked --no-fail-fast` 为 **708 passed、12 ignored、0 failed**（27 个测试二进制）；`cargo fmt --all --check` 干净；`cargo build --release -p neo-drawing -p neo-blackboard --locked` 成功。环境：Windows x64 MSVC，rustc/cargo 1.98.0。该构建不作为分发资产。

#### 笔宽平滑与擦除断片宽度连续

- 由书写速度推导的笔宽改为基于逐段速度的 ±30ms 时间窗移动平均，应用两遍（等效三角核，支撑 ±60ms，中心权重更高），而非原始瞬时采样：鼠标/触控的时间抖动会被基础笔宽成倍放大成粗笔下的锯齿边缘。时间窗使实时预览（稀疏事件采样）与提交后的重采样笔划获得同样的平滑尺度，只削高频抖动、不抹慢快趋势，真实停顿与刻意快慢切换不被平均。
- 笔划首点宽度改取第一段的速度，不再重置为最粗值：擦除后幸存碎片不再弹回满基准宽——碎片起点的宽度与切断处的书写速度一致，真实慢起笔依然粗起。
- 两份宽度实现（board-ink 与渲染层副本）同步修改，相等性由既有测试守卫；新增回归测试覆盖抖动平滑（匀速加交替事件间隔在 40 号笔下相邻宽度差收敛到 2px 内）与碎片起点连续（中段碎片保持局部 25px 宽而非重置 40px）。
- 验证：`cargo test --workspace --locked --no-fail-fast` 为 **705 passed、12 ignored、0 failed**（27 个测试二进制；本轮改动前 703 加两个新宽度测试），含 board-render 全套与百万点渲染测试；`cargo fmt --all --check` 干净。时间窗实现的第一版在长时间戳平台（百万点测试全部同刻）上有 O(n²) 最坏情况导致测试爬行；滑动窗口已改用运行和，严格 O(n)——百万点测试约 1.5s 完成。

#### 无法识别算式时不显示墨迹按钮

- 识别或 LaTeX 转换失败时，墨迹候选不再显示纠正按钮（也没有可点的计算按钮）：无法识别的笔迹不显示任何墨迹按钮。已识别候选的计算器/曲线图标与“计算中”忙碌指示不变；数学面板仍可手动输入，失败提示改为指向面板而非纠错图标。
- 移除了 `Icon::Correct` 变体及其绘制；画布循环改为跳过无法识别的候选，不再渲染纠正按钮。
- 验证：应用审查后的补丁脚本 `.dbg/apply_gui_patch2.py` 后，`cargo test --workspace --locked` 应为 **703 passed、12 ignored、0 failed**（三处断言改为对失败识别要求 `ink_action().is_none()`；畸形 LaTeX 预览测试更名为 `malformed_latex_shows_no_ink_buttons_and_keeps_preview`）。应用时由维护者实跑并将实际数字记入本节。

#### 答案落位匹配所写的式子

- 墨迹候选结果的字号取来源墨迹逐笔高度约 80 分位（等号、分数线、点号等矮笔划不会拉低估计，多行手势仍估出行高），钆 14..400，空回退 26；不再固定 26 号。
- 手写答案逐笔宽度改为来源手势的笔宽（钆 0.1..100）；无来源笔宽时保留学习风格宽度。
- 落位：等式结果放来源下方左对齐；其余结果放右侧并垂直中心对齐来源；间距随来源高度缩放（0.4×，钆 14..48）。答案包围盒不可量时保留旧锚点位置。
- 验证：features 单元测试覆盖尺寸分位、居中、间距、笔宽覆写与退化包围盒；锁定旧固定落位的 4 处 GUI 测试已改为新契约。gui.rs 的本轮改动因工作区写守卫误报该文件既有 `authorization` 字段行而由审查后的脚本 `.dbg/apply_gui_patch.py` 应用：运行一次后执行 `cargo test --workspace --locked` 与 `cargo fmt --all`。

#### 更顺畅的个人笔迹运动与两个新增学习维度

- 合成笔迹答案现在有笔势：单笔时间戳采用有界缓入缓出包络，单笔总时长保持 `路径长 / 速度` 不变，学习到的速度仍定快慢，起收笔变慢、中段变快。分数横线与根号等结构墨共用同一运动；手工样本保持原几何与原时序。
- 仅字体骨架回退字形（模板与手工样本不动）经单趟有界平滑，像素细化锯齿更少。平滑强度跟随学习到的转角锐度；超过 2,048 点预算的超大输入原样保留。
- 自动风格学习每笔新增两个有界标量：中段峰值/整笔平均的速度比（1.0..2.2，驱动缓急强度）与转角锐度（0..1，驱动回退平滑强度）。观测窗口仍为 31，观测仍不保存无标签点。原计划的闭合环拒绝过滤在实现前放弃：手写的 `o`/`0`/`e`/`8` 本身就是闭合环，而滚动中位数本就限制了偶发图形笔划的影响。
- 档案文件现写格式 **3**，可读格式 **1/2/3**；旧档加载时新运动维度取默认值（速度峰值 1.35、转角 0.5）。格式 1 仍不允许携带学习风格。旧程序拒绝格式 3 档案，与其拒绝更新版本文档的行为一致。
- 手写预览路径应用学习到的转角锐度；数学面板答案路径目前仍调用两参数回退入口，使用默认值 0.5，待其调用点采用学习值。
- 验证记录：`cargo test --workspace --locked` 为 **701 passed、12 ignored、0 failed**（本次改动前基线：697 passed、12 ignored、0 failed）；`cargo fmt --all --check` 通过。两条编码旧档案格式版本的断言随格式变更同步更新（`version == 2` 改为 `3`；非法版本探测列表用 4 替换 3），未弱化其他测试。真机上的手写可读性验收与此前一样仍未完成。

#### Runtime adapter 兼容准备

- 修复 hosted 模式 configure 前 close：原生窗口尚未创建时，由启动适配器完成已接受的隐藏/关闭请求，不再等待下一帧 stdin 或 EOF。已配置会话仍须真实窗口确认，dirty 保护和截图停止屏障不变。
- 新增15项无窗口启动回归与19项既有协议兼容测试，覆盖两种应用、严格配置、双向能力、取消/撤权、dirty close、EOF、资源分块及手写v3无损持久化。JSON Lines仍为v1；不新增Mod manifest、grant、generation字段或RPC。官方注册与进程代次隔离仍由主adapter负责。
- 已验证 stdin 保持打开的回归在修复前失败。本轮修复后完整工作区测试 **697通过、12忽略**，格式与差异检查通过；不宣称原生GUI/截图验收完成。这是8c32db0之后的独立改动，不修改主Neo v0.1.0标签或其runtime锁定版本。

#### 个人笔迹答案与修复

- 新增实验性本地个人风格答案，默认开启，与仍默认关闭的 HWR 独立。仅新提交的本地画笔笔划更新有界数值风格统计；精确字形样本仍是可选、人工标注流程。计算/写板仍须点击，不个性化宿主 Agent 回答；不新增神经训练、模型下载或 RPC。
- 修复输入与结果接收顺序：同帧键盘编辑、Esc 及相关指针/设置变更先于已经就绪的后台结果处理。持续按住输入不再无限阻塞结果排空；采样草稿存在且文本输入未聚焦时，采样区 Ctrl+Z 优先于文档撤销。
- 合成分数横线与运算符使用共同数学轴；字距计入限幅笔宽及有界斜接（miter）外扩。保留小写字母尺寸，使 `o` 小于 `0`，并使用区别于 `×` 的带钩 `x`；不保证所有手写字形均无视觉歧义。
- 笔迹预览生成与档案 JSON I/O 改为共享单槽后台执行、无排队；相关上下文变化后丢弃过期预览/加载结果，空闲帧复用预览网格。预览不修改文档或学习统计。
- 档案保存/加载仍须明确本地路径及覆盖/替换确认。已授权保存异步写入点击时快照：关闭面板或修改档案不会中断已开始的写入，也不包含之后的修改；无自动档案持久化或加载。
- 未应用个人风格的字体骨架与失败结果按已加载字体代次缓存，同代次快照共享；最多128字符、65,536点、2 MiB计账条目负载。成功替换字体开启新代次，尚存快照可保留旧代次。缺字或超预算时整个答案保留标准渲染并提示诊断，不生成半份个人笔迹。

#### 直线求点、渲染与持久化

- 图像“求点”扩展到 `y=2x+1`、`f(x)=2x+1`、`y-x=1` 及手动绘制的 `x=2` 等直线，支持坐标轴交点和与受支持显函数混合求交。平行线不产生两线交点；重合线与矩形范围共有线段时报告非离散交点，仅接触矩形一个角点时返回该单点。
- 两直线使用解析公式，但运算仍为 **f64浮点数**，不是精确有理数算法。近奇异系统、求值或残差校验失败可报错。竖线与显函数在固定 x 求值；其余显函数对仍使用有界、不完备的数值搜索，候选按 x/y 两轴范围裁剪。
- 修复边界裁剪：最多吸收4 ULP的浮点舍入，保留 `0.1*x+0.2` 在 `x=1`、`y_max=0.3` 等小数边界交点。钳制后的点须通过机器精度级残差校验，不使用求根容差扩大矩形；数值搜索候选也重新校验双方原始表达式AST，保留其定义域及算术错误检查。
- 一般隐式二次曲线仍不支持“求点”。所检查的前16条曲线中出现不支持项时不执行整图搜索，不将显函数子集冒充完整结果；更多曲线给出上限诊断。不新增 RPC，也不扩展 `math.calculate`。
- 已提交页面采用文档 revision 渲染缓存，临时手势预览单独处理。每个页面渲染器保留的网格顶点/索引缓冲（含聚合批次）预算为64 MiB；这**不是全局 RAM 上限**，来源快照、场景、资源、已提交帧、其他缓存及应用状态另计。GUI 字体文件读取限制为64 MiB，安装前校验字体数据。
- 文档/资源包读取 v1/v2/v3，按内容选择最小保存版本：含 `Handwritten` 为v3，否则含 `Math` 为v2，其余为v1；图片仅决定资源打包。冻结整答案笔划、标准文字及可选数学布局重开无需档案，旧v1/v2程序不能读取v3。档案JSON版本独立，JSON Lines仍为version 1。

#### 验证记录与待完成验收

- **最终实跑记录（维护者提供）**：`cargo test --workspace --locked --quiet` 为 **663 passed、12 ignored、0 failed**；`cargo fmt --all --check` 与 `git diff --check` 通过。`neo-drawing` 和 `neo-blackboard` 的 release 构建均成功，耗时 **2分23秒**。ignored 测试不计为通过。下方v0.0.1测试/构建记录保持历史性质；本轮仅更新文档，未重新运行这些测试或构建。
- 键盘/取消顺序、预览/档案流程、手写数学与复杂汉字可读性、直线求点仍待原生GUI人工验收；目标i5/3 GB响应速度及整机内存测量仍待完成。有界缓存不证明目标机器达标。
- 本节不宣称已创建发布/标签、已公开发布或新增任何许可/来源核查结论。

## v0.0.1 — Initial source prerelease

### English

#### Release scope

- Two Rust desktop applications: Drawing (`neo-drawing`) for transparent screen annotation and Blackboard (`neo-blackboard`) for multipage writing, plots, and limited mathematics.
- Source-only distribution in `ChidcGithub/NeoRuntime-drawing`, using GitHub's automatic source ZIP/tar.gz archives. No executable, installer, model weights, font package, dependency bundle, or runtime package is attached.
- Binary and model distribution is deferred because third-party license/attribution and runtime-distribution review is incomplete. Building locally may download crates and ONNX Runtime inputs; source-only does not mean dependency-free or guaranteed offline.
- Project-owned workspace packages use [Apache-2.0](LICENSE). Third-party components retain their own licenses; no copyright holder or third-party redistribution clearance is invented by these notes.
- Application version is 0.0.1; JSON Lines protocol remains version 1. The current GUI, help, and many status/error messages are Chinese.

#### Writing, menus, selection, and resizing

- Pens with color, width, and dash controls; erasing; lines and shapes; selection and geometry editing; undo/redo; and page management for up to 500 pages.
- The adaptive toolbar keeps its main controls on one row and moves secondary groups into More on narrow windows. Pen/eraser buttons first select the tool and then open settings on another click, without a timed double-click requirement. Other menus open with one click; secondary menus open upward with bounded scrolling.
- Drag erasing previews changes without modifying document revision/history and commits a valid gesture as one undo step. Cancellation, stale context, or a miss does not create an edit.
- Clear all in the eraser menu removes every object and connection on the current page as one undo step, without changing other pages or page count. An empty page adds no history entry.
- Line endpoints can connect to supported shape vertices/edges and follow target edits. Connections participate in document persistence and undo/redo.
- Images, including screenshots, and function plots have four selection corner handles. The opposite corner stays fixed; images preserve aspect ratio, while plot-frame width and height can change independently without changing expressions or coordinate ranges.
- Resize uses live preview and a single commit on release, with minimum-size and finite-coordinate checks. Stationary or stale gestures do not commit. Resizing does not trigger image questions or plot merging; wheel zoom over a selected plot remains a separate coordinate-range operation.
- Drawing collapse hides its toolbar/panels but keeps ink visible. Blackboard collapse reduces the same native window to a small expand/exit control. Both retain the document/history; neither collapse is proof that all windows are hidden for capture.
- Windows Drawing defaults to Glow/OpenGL; Blackboard and other platforms default to wgpu. `NEO_DRAW_RENDERER=glow` or `wgpu` overrides the choice, without automatic fallback.

#### Built-in Windows capture and host Agent integration

- Standalone screenshot capture uses an application-owned Win32 region-selection overlay and GDI pixel acquisition. It does not invoke Snipping Tool, read/write the clipboard, or automatically upload images.
- Clicking the screenshot action authorizes that one local capture. Hide window during capture defaults to on and can be disabled for the current run; keeping the board visible may include it and its ink in the image.
- Region selection supports cancellation by Esc, right-click, focus loss, or display-layout change. Capture has a cooperative 60-second soft budget, not a hard deadline. Cleanup waits for the local thread/overlay to exit before releasing its hide lease and restoring the prior visibility intent; no external-tool confirmation dialog is required.
- Pixels are read after selection finishes, not from a frozen desktop image. A valid result is automatically inserted and selected as one undo step only while the original document/page/revision remains valid.
- Clicking an image opens its Agent authorization panel without sending data; dragging still moves it, and corner handles resize it. Agent requests require a separate Neo host. Image sending and answer write-back require separate consent, with write-back off by default. Ordinary answer text is not automatically converted into board objects.
- Hosted capture requires host permission, classroom-safe mode off, and confirmed hiding of every owned window. Local capture options neither bypass these checks nor change Session permissions or introduce an RPC.
- Session handling supports immediate host results or a host job ID followed by a completion event, bounded resource transfer with integrity checks, cancellation, stale-result rejection, and permission-aware atomic write-back. Host implementations are not included.
- GUI host tasks offer manual cancellation and request cancellation after 60 seconds. A cancellation request or disconnect does not itself confirm host capture has stopped; hiding leases wait for the required confirmation. Session/headless exposes no general task timeout or automatic reconnect method.

#### Bounded mathematics and plots

- Blackboard provides explicit manual calculation, result insertion, plotting, coordinate bounds, numerical derivatives, and finite-interval numerical integration. Drawing does not gain a mathematics panel.
- Constant calculations support bounded exact fractions and square roots with checked integer arithmetic. Unsupported exact operations may fall back to approximate constant results marked with `≈`; domain errors, overflow, and exhausted budgets are errors, not silent approximations.
- Fraction/radical results can be inserted as two-dimensional math objects with shared screen/PNG/SVG layout. Manual input stays in the panel; recognized source ink stays on the board. Each inserted result is one undo step.
- Polynomial simplification, differentiation, and antiderivatives are limited to total degree at most 12 and use the existing floating-point path. One-variable linear/quadratic equations and two-variable linear systems are supported; this is not a full computer algebra system or arbitrary-precision solver.
- GUI quadratic-system, root, and intersection searches are bounded numerical searches. “No candidate found” is not proof of no solution. GUI bounds/numerical controls do not expand the `math.calculate` RPC, which returns formatted text without inserting an object.
- Plots support explicit functions and a single implicit x/y polynomial equation with numeric coefficients and total degree at most two, such as `y^2=x` or `x^2+y^2=9`. Arbitrary implicit, higher-degree, and parametric plotting are unsupported. Implicit or mixed explicit/implicit plots do not support intersection search.
- 3D shapes are two-dimensional projected wireframes, not rotatable solid geometry or a 3D constraint solver.

#### Opt-in handwriting recognition and manual correction

- Handwriting recognition is off by default and enabled explicitly for the current run. A 2.5-second writing pause triggers recognition only, not automatic calculation, plotting, panel opening, or write-back; it is not a completion deadline.
- The default offline template backend is limited to supported symbol/layout patterns, not arbitrary handwriting. Personal symbol templates can be learned, saved, and loaded through explicit local JSON paths; this is not general model training.
- Optional TexTeller recognition uses local Rust/ONNX Runtime CPU inference. Model weights must be prepared separately, then explicitly selected by absolute directory and loaded/reloaded. The GUI neither downloads nor automatically discovers and loads models; ordinary drawing does not require weights.
- The optional model-preparation download is approximately 1.25 GB and needs network access. Python is used for preparation, not as an inference subprocess. Recognition being opt-in does not remove ONNX Runtime from the current build dependency graph.
- Candidates require review and a click on the calculate/plot icon. The correction control opens the original input for editing without calculating; the user explicitly confirms corrected input before calculation/plotting. Unsupported or ambiguous LaTeX is rejected rather than silently stripped.
- Recognition scores/log probabilities are not calibrated correctness probabilities or authorization for automatic calculation. Busy tasks cannot be submitted repeatedly; soft budgets and cooperative cancellation do not forcibly interrupt an ONNX Runtime call.
- Candidate caches are bounded, in memory, and scoped to document/page. Source or nearby-content changes invalidate affected candidates; document replacement clears the cache. Existing results suppress duplicate insertion. Background writes still require the original document/page/revision, even when a locally cached candidate remains valid.

#### Persistence, recovery, export, and protocol

- Documents save referenced PNG assets and connections. Opening validates the replacement before changing the current document; saving uses a same-directory temporary file and rename. Failed saves leave the document dirty.
- New/open/exit protect unsaved changes. Page PNG/SVG export is neither a document save nor a desktop screenshot. PNG export requires suitable fonts and image resources; SVG without supplied font outlines depends on viewer fonts.
- Document formats v1 and v2 are readable. Two-dimensional math objects require v2, which older v1-only readers cannot read. Images use resource packages; implicit plots alone do not change the file version. Undo history, recognition candidates, permissions, window state, and pending tasks are not persisted.
- Disconnection attempts recovery saving under `NeoRuntime-drawing/recovery` in `LOCALAPPDATA`, falling back to the system temporary directory. Paths/failures go to stderr. Recovery files are not automatically scanned or reopened, and disk errors can prevent recovery.
- No arguments/help/version open no GUI. `--gui`, `--gui --hosted`, and `--headless` provide the implemented launch modes. Hosted GUI waits for configuration before creating a native window; help/version/logs use stderr and stdout is reserved for JSON Lines.
- Protocol v1 uses bounded JSON Lines frames, document-level revisions, atomic object operations, paginated/chunked reads, PNG resource checks, and persistent connections. Clients must inspect actual `ready.methods`; GUI/library features do not imply new RPCs. The protocol and API reference are now maintained in the local-only `api/` directory, excluded from current public checkouts.

#### Fixes included in the initial baseline

These are fixes made during initial development, not regressions relative to a previous public release.

- Stationary line-endpoint clicks no longer spuriously snap geometry or add an undo entry.
- Segment interpolation preserves exact endpoints across large differences in coordinate magnitude. Final scaling of a nonzero numerical integral that underflows is reported as an error rather than silently returning zero.
- Duplicate original host responses are ignored after capture enters the asynchronous host-job stage; cancellation does not prematurely release capture hiding.
- A host resource descriptor sharing a name with a local asset still requires host download, validation, and cleanup rather than substitution of the local image.
- `close`, `document.new`, and `document.open` validate `discard_unsaved` as a Boolean.
- Targeted regressions cover menu/clear-all history, frame-resize handles and context validity, accidental merge/question prevention, capture selection and cancellation cleanup, and authorization boundaries. These tests do not establish that all bugs have been found.

#### Validation record and known limits

- **Prepublication verification:** `cargo test --workspace --locked --offline` completed with **540 passed, 12 ignored, 0 failed**. `cargo build --release -p neo-drawing -p neo-blackboard --locked --offline` succeeded. Environment: Windows x64 MSVC, rustc 1.97.1, Cargo 1.97.1.
- The dormant local HTTP debug collector was removed before publication; runtime diagnostics remain opt-in and local. Model weights, build output, user documents, credentials, logs, and internal working notes are excluded from the tracked source tree. Ignored model/GPU/performance tests were not run and are not counted as passed.
- No complete native GUI, actual desktop-capture, mixed-DPI/multimonitor, touch, clean-machine runtime/distribution, or real Neo-host acceptance is claimed. Synthetic capture and pointer tests do not substitute for native manual validation.
- GDI capture may omit protected content or mishandle HDR. Not all stripped-down Windows installations or other platforms are supported equivalently. Font availability affects display/export.
- Computation, resources, history, and recognition have explicit limits. Mathematical searches are not complete solvers, recognition requires review, recovery is best-effort, and renderer fallback is not automatic.
- There is no pinned Rust toolchain. Windows source builds require an edition 2024-compatible MSVC Rust toolchain and native build tools; `--locked` does not pin the compiler or all native build inputs.

### 中文

#### 发布范围

- 首个源码型预发布，包含透明屏幕批注应用 Drawing（`neo-drawing`）和支持分页书写、函数图像及有限数学功能的 Blackboard（`neo-blackboard`）。以下功能和修复描述初始版本所含实现，不是相对某个此前已公开版本的变更。
- 面向 `ChidcGithub/NeoRuntime-drawing`，仅使用 GitHub 自动生成的源码 ZIP/tar.gz；不附加可执行文件、安装包、模型权重、字体包、依赖合集或运行库包。
- 第三方许可、署名/NOTICE 和运行库分发核查尚未齐备，暂缓二进制与模型分发。本地构建仍可能下载 crates 和 ONNX Runtime；“仅源码”不等于无需依赖、保证离线或所有依赖均从源码构建。
- 自有 workspace 包采用 [Apache-2.0](LICENSE)，第三方组件保留各自许可。本文不编造版权人，也不宣称第三方分发手续已完成。
- 应用版本为 0.0.1，JSON Lines 协议仍为 v1。当前 GUI、命令行帮助及许多状态/错误消息为中文。本文档不代表标签或 GitHub Release 已实际发布。

#### 书写、菜单、擦除与尺寸调整

- 提供画笔颜色/粗细/虚线、橡皮、直线与形状、选择和几何编辑、撤销/重做，以及最多 500 页的页面管理。
- 自适应底部工具条保持单行，窄窗口将次要分组收入“更多”。画笔/橡皮首次单击切换工具，再次单击打开设置，无快速双击时限；其他菜单单击打开，二级菜单向上展开并限制高度、支持滚动。
- 拖动擦除实时预览，不在预览阶段修改文档 revision 或历史；有效松手一次提交，整段手势一个撤销项。取消、上下文过期或未命中不产生编辑。
- 橡皮菜单“全部擦除”一次清空当前页全部对象及连接，保留其他页和页数，支持单次撤销/重做；空页不新增历史。
- 直线端点可连接支持的形状顶点/边并随目标更新，连接参与保存和撤销/重做。
- 图片（含截图）和函数图支持选择后的四角尺寸手柄：固定对角，图片等比缩放，函数图宽高独立调整，但不改变表达式或坐标范围。
- 缩放实时预览、松手一次提交，检查最小尺寸与有限坐标。静止或过期手势不提交；拖手柄不误询问 Agent，也不触发函数图合并。选中函数图的滚轮坐标范围缩放仍是独立操作。
- Drawing 收起仅隐藏工具栏/面板，保留笔迹；Blackboard 收起将同一原生窗口缩为展开/退出入口。两者保留文档及历史，收起不等于确认全部窗口隐藏。
- Windows Drawing 默认 Glow/OpenGL，Blackboard 及其他平台默认 wgpu；可用 `NEO_DRAW_RENDERER=glow` 或 `wgpu` 覆盖，无自动后端回退。

#### 内置 Windows 截图与宿主 Agent

- 独立 GUI 截图使用应用自有 Win32 框选层和 GDI 像素采集，不调用系统截图工具，不读写剪贴板，不自动上传图片。
- 单击截图即授权本次本地采集。“截图时隐藏窗口”默认开启，仅本次运行有效；关闭后窗口及笔迹可能进入截图。
- Esc、右键、失焦或显示器布局变化可取消选区。流程有 60 秒协作式软预算，不是硬截止；等待本地线程/框选层退出并完成清理后，才释放本次隐藏租约、恢复此前显隐意图，无外部工具恢复确认框。
- 选区结束后才获取像素，并非冻结桌面快照。仅原文档/页面/revision 仍有效时自动插入并选中截图，一次撤销。
- 单击图片内部仅打开 Agent 授权面板，不发送；拖动移动，角手柄调整尺寸。Agent 需要独立 Neo 宿主服务，图片发送和回答写回分别授权，写回默认关闭；普通回答文字不会自动转为板书对象。
- 宿主截图仍需宿主权限、关闭课堂安全模式并确认全部所属窗口隐藏。本地选项不放宽这些条件、不改变 Session 权限、不增加 RPC。
- 会话支持宿主直接完成或先返回任务 ID 再发完成事件，以及有界图片分块传输/完整性校验、取消、过期结果拒绝和权限约束下的原子写回；本仓库不包含宿主服务实现。
- GUI 宿主任务可手动取消，等待 60 秒后也会请求取消。请求取消或断连不证明宿主已经停止采集，隐藏租约仍需相应确认；Session/headless 没有通用任务超时或自动重连接口。

#### 有界数学与绘图

- 黑板提供手动计算、结果写板、绘图、坐标范围、局部数值导数及有限区间定积分。透明画板没有数学面板。
- 常量计算支持有界精确分数/平方根和 checked 整数运算；不支持的精确运算可回退为带 `≈` 的常量近似结果。定义域错误、溢出和超预算直接报错，不静默近似。
- 分数/根式结果可保存为二维数学对象，屏幕与 PNG/SVG 共享布局。手动输入留在面板，手写原笔迹留在板上；新增结果一次撤销。
- 多项式化简、微分和原函数限制总次数不超过 12，仍采用原有浮点路径；支持一元一次/二次方程及二元线性方程组，不是完整 CAS 或任意精度求解器。
- GUI 二元二次系统、根和交点搜索均为有界数值搜索，“未找到候选”不等于“无解”。GUI 范围/数值控件不扩展 `math.calculate` RPC；该方法只返回格式化文字，不自动插入对象。
- 绘图支持显函数及数值系数、总次数不超过二的单个 x/y 隐式多项式方程，例如 `y^2=x`、`x^2+y^2=9`。不支持任意隐式、高次或参数方程绘图；隐式及显隐式混合图不支持交点搜索。
- 三维形状只是二维投影线框，不是可旋转实体或三维约束求解器。

#### 默认关闭的手写识别、可选模型与人工纠错

- 手写识别默认关闭，需本次运行显式启用。停笔 2.5 秒只触发识别，不自动计算、绘图、打开面板或写回，也不是完成时限。
- 默认离线模板后端只适用于支持的符号/布局，不是通用手写识别。可通过明确的本地 JSON 路径学习、保存和加载个人符号模板，不是训练通用模型。
- 可选 TexTeller 使用本地 Rust/ONNX Runtime CPU 推理。权重单独准备，指定绝对目录后显式加载/重载；GUI 不下载、不自动发现并加载模型，普通绘画无需模型权重。
- 可选模型准备下载约 1.25 GB，需要网络；Python 用于准备，不作为推理子进程。识别功能可选，不等于当前构建依赖图已移除 ONNX Runtime。
- 候选须核对后单击计算器/曲线图标；纠错入口只打开原输入供编辑，不执行计算，修改后仍需显式确认。未知、歧义或不支持的 LaTeX 完整拒绝，不静默删除后继续计算。
- 相似度/log probability 不是校准的正确概率或自动计算授权。忙碌时不能重复提交；软预算和协作取消不会强杀正在执行的 ONNX Runtime 调用。
- 候选按文档/页面保存在有界内存缓存中，源笔迹或邻域内容变化可使其失效，替换文档清空缓存；已有结果抑制重复写入。后台写回仍严格匹配原文档/页面/revision，局部候选有效也不放宽此检查。

#### 文档持久化、恢复、导出与协议

- 保存引用的 PNG 资源及连接。打开先完整校验再替换当前文档；保存采用同目录临时文件再 rename，失败保留 dirty 状态。
- 新建/打开/退出保护未保存修改。页面 PNG/SVG 导出不是文档保存或桌面截图；PNG 文字依赖合适字体与图片资源，未提供字体轮廓的 SVG 依赖查看器字体。
- 支持读取文档 v1/v2。二维 Math 对象要求 v2，旧的仅 v1 阅读器不能读取；图片使用资源包，隐式图本身不升级文件版本。撤销历史、识别候选、权限、窗口状态及未完成任务不写入文档。
- 断连时尝试在 `LOCALAPPDATA` 下的 `NeoRuntime-drawing/recovery` 保存恢复文件，缺失时回退到系统临时目录；路径或失败写 stderr。不自动扫描/重开，磁盘错误可能导致恢复失败。
- 无参数/help/version 不启动 GUI。支持 `--gui`、`--gui --hosted`、`--headless`；hosted GUI 等待配置后才创建原生窗口。帮助/版本/日志使用 stderr，stdout 仅用于 JSON Lines。
- 协议 v1 实现有界帧、文档级 revision、原子对象操作、分页/分块读取、PNG 校验和持久连接。调用方应检查实际 `ready.methods`；GUI/库功能不自动等于新增 RPC。协议和 API 参考现保留在本地 `api/` 目录，不包含在当前公开检出中。

#### 初始开发阶段已纳入的修复

以下是首个发布基线所含修复，并非相对某个此前公开版本的回归修复清单。

- 静止单击线段端点不再误吸附、改变几何或新增撤销项。
- 大数量级差异下的线段插值保留真实端点；非零数值积分最终缩放下溢报错，不静默返回零。
- 截图进入宿主异步任务阶段后忽略原请求重复响应，取消期间不提前解除隐藏。
- 宿主资源描述符与本地资源同名时仍执行宿主下载、校验和清理，不替换为本地图片。
- `close`、`document.new`、`document.open` 严格检查 `discard_unsaved` 布尔类型。
- 针对菜单/清页历史、四角缩放及上下文有效性、防误合并/误询问、截图选区与取消清理、授权边界提供回归测试，不代表已穷尽全部 bug。

#### 验证记录与已知限制

- **发布前实跑验证**：`cargo test --workspace --locked --offline` 为 **540 passed、12 ignored、0 failed**；`cargo build --release -p neo-drawing -p neo-blackboard --locked --offline` 成功。环境为 Windows x64 MSVC、rustc 1.97.1、Cargo 1.97.1。
- 发布前移除了旧本机 HTTP 调试上报器；运行时诊断仍为主动启用的本地输出。模型权重、构建产物、个人板书、凭据、日志和内部工作记录均排除在推送源码之外。ignored 模型/GPU/性能测试未运行，不计为通过。
- 不宣称完成原生 GUI、真实桌面采集、混合 DPI/多屏、触控、干净机器运行库/分发或真实 Neo 宿主验收；合成截图/指针测试不能替代原生人工验证。
- GDI 对受保护内容/HDR 有限制，不保证所有精简 Windows 或其他平台行为一致；字体供应影响显示与导出。
- 计算、资源、历史及识别均有界。数值搜索不保证完备，识别需人工核对，恢复尽力而为，渲染器无自动回退。
- Rust 工具链未固定。Windows 源码构建需支持 edition 2024 的 MSVC Rust 工具链及原生构建工具；`--locked` 不固定编译器或全部原生构建输入。
