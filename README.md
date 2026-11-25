# AI Token HUD

<div align="center">

[![Release](https://img.shields.io/badge/Release-v0.2.3-0ea5e9?style=flat-square&logo=github)](https://github.com/WuDuKangBaZi/ai-token-hud/releases/tag/v0.2.3)
![Rust](https://img.shields.io/badge/Language-Rust-orange?style=flat-square&logo=rust)
![Platform](https://img.shields.io/badge/Platform-Linux%20Desktop%20%7C%20Windows%2010%2F11-blue?style=flat-square&logo=linux)
![GUI](https://img.shields.io/badge/GUI-eframe%20%2F%20egui-purple?style=flat-square)
![License](https://img.shields.io/badge/License-MIT-green?style=flat-square)

**桌面悬浮的 AI Token 与费用实时监视器 · 多维统计工作台**

一条 38px 高的置顶 HUD 悬浮横条，内置 **Modern Glass（极客暗色玻璃）** 与 **Cyber OSD（硬核遥测监视器）** 双视觉主题。  
实时监控 Claude Code、Codex、Antigravity、GitHub Copilot、Gemini 等 AI 编程工具的输出流速、当天花费、周期累计与缓存命中率。  
基于操作系统原生文件事件驱动刷新（Linux `inotify` / Windows `ReadDirectoryChangesW`），空闲时零轮询、0% CPU 占用与极低磁盘 I/O，内存自动修剪至 ~15–30 MB。

[前往 GitHub Releases 下载 v0.2.3 绿色免安装版](https://github.com/WuDuKangBaZi/ai-token-hud/releases/latest)

</div>

<div align="center">

![HUD Bar](https://cdn.jsdelivr.net/gh/WuDuKangBaZi/ai-token-hud@master/assets/hud-bar.png)

> 上图及下文的金额均为示例数据。
</div>

---

## 核心功能

- **双视觉主题与原生窗口特效**：
  - 随时在 **极客暗色玻璃（Modern Glass）** 与 **硬核遥测监视器（Cyber OSD）** 间切换，采用全自绘矢量图标、跨平台分层字体栈（Linux `Ubuntu / Noto Sans / JetBrains Mono / Noto Sans CJK`；Windows `Segoe UI / Cascadia Mono / 微软雅黑`）与千分位等宽数字。
  - 支持桌面合成器透明圆角（Windows 11 DWM 亚克力 / Linux X11 & Wayland 合成器透明），以及 **鼠标点击穿透模式（`Ctrl+Alt+T`）**，不遮挡底层 IDE 或全屏操作。
- **HUD 实时悬浮横条（全中英文国际化）**：
  - 中文模式下显示 **速率 (`T/s`)**、**当天**、**全部/本周/本月**、**缓存命中 (`%`)**，并在 `速率` 右侧绘制最近请求 Sparkline 实时速率波形图；支持内联可选双币种副金额。
  - 三种展示模式：**按工具**（仅显示当天活跃使用的工具）、**按模型**（仅显示当天实际调用的模型）、**仅金额**（搭配自由开关周期统计）。
  - 支持 `TOTAL` 统计范围切换（**本周 / 本月 / 全部**）及五方位屏幕停靠预设（**左上角 / 居中 / 右上角 / 左下角 / 右下角**）与边缘磁吸（避让任务栏）；切换模式或范围时自动校准位置，底部停靠时面板与右键菜单自动**向上**展开。
- **三合一可固定遥测展开面板**：
  - 默认通过点击右侧展开 / 固定按钮或全局快捷键 `Ctrl+Alt+F` 展开面板（亦可在设置中开启「悬停 HUD 自动展开」），支持拖拽外缘抓手或设置滑块**自定义面板高度（200–720 px）**；展开后横条自动拉伸至与面板同宽，核心指标对称居中并均匀分布分隔线留白。
  - **多周期矩阵**：横向对比各模型在今日、本周、本月、上月、累计五个周期的费用，零磁盘 I/O 增量更新。
  - **今日分布**：展示输入 / 输出 / 缓存读取占比堆叠条、Prompt 缓存省钱总额与工具消耗排行。
  - **实时请求流**：按时间倒序展示单次请求明细，Tokens 拆分为 **输入 / 输出 / 缓存创建 / 缓存命中** 四列，联动真实生成速率与高额消费标红提醒。
- **预算与异常处理**：
  - 支持 **每日预算** 与 **月度预算**（HUD 底部实时绘制进度条，≥80% 橙色预警，超支标红）及 **单次高额请求标记阈值**；过滤低于 100ms 与异常毛刺耗时，超过 60 秒无请求指示灯自动置灰。
- **模型单价、在线目录同步与双币种汇率**：
  - 内置 2026 主流大模型官方单价表（Claude、OpenAI/Codex、Gemini、DeepSeek、Qwen、GLM、EXAONE、Grok、MiniMax 等），支持一键同步 `models.dev` 与 OpenRouter 在线定价目录。
  - 支持行内修改或手动新增自定义模型，变更后**纯内存回溯重算历史缓存与所有报表**；支持 USD / CNY 实时汇率同步与开机自动更新。
- **原生与 CC Switch 双引擎驱动**：
  - 自动探测系统环境，支持智能切换或手动强制 **CC Switch SQLite** / **原生会话日志扫描**，底层采用嵌入式 **SQLite WAL 增量缓存库（`native_cache.db`）** 持久化游标、多日汇总、会话日明细与今日快照，跨零点与重启零数据丢失（旧版 `native_*.json` 首次启动自动无损迁移为 `.bak`），并支持工作台与右键菜单一键触发**后台非阻塞「全量重新扫描」**。
  - **极低磁盘 I/O**：`ReadDirectoryChangesW` 活跃期间零空闲目录轮询；原生缓存基于脏键追踪与 10 秒合并节流落盘，仅对发生变更的游标与归档行执行 O(1) SQLite 点写（彻底消除大 JSON 整包写放大），退出、最小化至托盘及跨零点自动立即刷盘并执行被动 WAL checkpoint。

---

## 界面示意

### 多周期明细面板

<div align="center">

![多周期明细面板](https://cdn.jsdelivr.net/gh/WuDuKangBaZi/ai-token-hud@master/assets/detail-panel.jpg)

</div>

### 独立统计与设置工作台

双击横条、点击 `📅` / 滑块图标或右键托盘打开，共 **6 个选项卡**（支持 `--settings <序号>` 直达）：

| 序号 | 选项卡 | 核心功能 |
| :--- | :--- | :--- |
| 0 | **日历与趋势** | 月历热力图、每日柱状趋势图、8 档预设时间范围 + 日历自选起止日期、可排序自适应模型明细表、**一键复制 TSV 与 UTF-8 BOM CSV 账单导出** |
| 1 | **消耗全景下钻透视** | **工具 / 模型双视角下钻透视**、顶部费用份额彩带、日历同款 8 档时间筛选 + 「同步日历自定义」筛选 |
| 5 | **项目与会话消耗排行榜** | 按项目/按会话排行、关键词过滤，精确关联 Antigravity / Copilot / Codex / Claude Code 的代码仓库路径与会话标题 |
| 2 | **模型单价与汇率** | USD/CNY 汇率在线同步、在线模型目录同步（`models.dev` / OpenRouter）、筛选/行内修改/手动新增自定义模型 |
| 3 | **外观与悬浮窗** | Modern Glass / Cyber OSD 预览切换、展示模式、`TOTAL` 范围、停靠位置、展开面板高度、DWM 模糊、点击穿透、全局快捷键、预算、开机自启动 |
| 4 | **数据源与工具** | 采集驱动策略（智能自适应 / 强制 CC Switch / 强制原生扫描）、内核监听路径状态、已安装工具探测与显隐开关 |

---

## 数据来源

统计支持两种驱动模式（默认自适应优先 CC Switch，未安装或强制原生时启用本地会话日志增量解析）：

| 数据源 | 读取位置 | 说明 |
| :--- | :--- | :--- |
| [CC Switch](https://github.com/farion1231/cc-switch) | `~/.cc-switch/cc-switch.db` | 只读 URI 打开，联合查询 `proxy_request_logs` 与 `usage_daily_rollups` |
| Claude Code | `~/.claude/projects/<dir>/*.jsonl` 与 `subagents/*.jsonl` | 增量游标解析 + 按 `message.id` 去重 + 同目录真实工作区路径（保留连字符） |
| Google Antigravity | `~/.gemini/antigravity/brain/`、`conversation_summaries.db`（含 `-cli`） | 增量扫描 `PLANNER_RESPONSE`，通过摘要库 + 父子链继承关联项目名与会话标题 |
| OpenAI Codex | `~/.codex/sessions/YYYY/MM/DD/*.jsonl` 及 `.jsonl.zst` | 增量读取与 `.zst` 解压索引、UUID 提取、`last_session_id` 游标续传 |
| GitHub Copilot CLI | `~/.copilot/session-state/<uuid>/events.jsonl` 与 `workspace.yaml` | 增量解析 `session.usage_record`，解析仓库/目录路径与会话标题 |

> `~` 即 `$HOME`（Windows 为 `%USERPROFILE%`）。原生模式会自动在 `~/.ai-token-hud/` 归档历史日数据与今日缓存。

---

## 隐私与安全

- 所有工具日志与 SQLite 数据库均以**只读方式**打开，绝不写入或篡改第三方会话目录。
- 全部使用统计与自定义配置仅保存在本机 `~/.ai-token-hud/`，不上传任何遥测数据或代码内容。
- 仅有的两项**可选联网请求**（均可在设置中关闭）：
  1. 汇率获取：Linux 使用纯 Rust `ureq` + `rustls`（Windows 使用 WinHTTP API），不拉起任何外部命令行进程，访问 `open.er-api.com` / `api.exchangerate-api.com`。
  2. 在线模型定价同步：同样通过 `ureq` + `rustls`（Windows 为 WinHTTP）访问 `models.dev` / `openrouter.ai` 获取公开模型目录单价。

---

## 编译与运行

### Linux 桌面版 (Ubuntu 22.04 / 24.04+ · X11 / Wayland)

环境要求：Rust 1.85+（Edition 2024）、基础 C 工具链（`build-essential`、`pkg-config`）。托盘采用纯 Rust `ksni`（StatusNotifierItem D-Bus），网络采用纯 Rust `ureq + rustls`，**无需安装 GTK3、libappindicator 或 OpenSSL 开发头**。

```bash
# 仅需基础编译工具与系统运行时库 (Ubuntu / Debian)
sudo apt update && sudo apt install -y build-essential pkg-config

git clone -b linux https://github.com/WuDuKangBaZi/ai-token-hud.git
cd ai-token-hud
cargo test
cargo build --release
```

快速启动或安装至用户桌面菜单 (`~/.local/bin/ai-token-hud` 与 `~/.local/share/applications/ai-token-hud.desktop`)：

```bash
./start-linux.sh
# 或安装到系统应用菜单：
bash scripts/install-linux.sh
```

> **X11 与 Wayland 说明**：
> - **系统托盘**：使用 Freedesktop StatusNotifierItem (`ksni`)，KDE Plasma 与 XFCE 原生支持；Ubuntu GNOME 桌面默认自带 Ubuntu AppIndicators 扩展（若精简环境未启用，请开启 `gnome-shell-extension-appindicator`）。
> - **全局快捷键**：X11 会话原生监听 `Ctrl+Alt+H / T / F`；Wayland 会话下出于桌面安全隔离限制，可在「系统设置 → 键盘快捷键」中绑定以下命令行 IPC 动作（零延迟控制已运行实例）：
>   - `ai-token-hud --toggle`（显隐 HUD）
>   - `ai-token-hud --click-through`（切换鼠标穿透）
>   - `ai-token-hud --flyout`（展开/固定遥测面板）
>   - `ai-token-hud --settings 0`（打开工作台指定页签）
> - **开机自启动**：在设置开启后写入 `~/.config/autostart/ai-token-hud.desktop`，移动可执行文件后自动校正 `Exec` 路径。

### Windows 版 (兼容保留)

环境要求：Windows 10 / 11 (x64) + Rust 1.85+（MSVC 工具链，Edition 2024）。

```bash
cargo test
cargo build --release
target/release/ai-token-hud.exe --settings 0
```

### 本地配置文件与缓存

| 文件 | 位置 | 内容 |
| :--- | :--- | :--- |
| `config.json` | `~/.ai-token-hud/` | 主题、停靠位置、面板高度、`TOTAL` 范围、汇率、自定义单价、驱动与工具开关 |
| `history_cache.json` | `~/.ai-token-hud/` | T-1 历史统计加速缓存（采用 `.tmp` + rename 原子写入） |
| `native_cache.db` | `~/.ai-token-hud/` | 嵌入式 SQLite WAL 增量缓存库（存储文件游标、多日汇总、会话日明细与今日快照；旧版 `native_*.json` 首次启动自动迁移为 `.bak`） |
| `pricing_catalog.json` | `~/.ai-token-hud/` | 本地缓存的在线模型定价目录 |

删除以上文件即可恢复出厂初始状态。开启「开机自动启动」在 Linux 写入 `~/.config/autostart/ai-token-hud.desktop`（Windows 写入 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\AiTokenHud`），卸载前在设置关闭该开关即可清理。

---

## 操作与快捷键速查

| 操作 / 快捷键 | 效果 |
| :--- | :--- |
| `Ctrl+Alt+H` | 全局显示 / 隐藏 HUD 悬浮条 |
| `Ctrl+Alt+T` | 全局开启 / 关闭鼠标点击穿透模式 |
| `Ctrl+Alt+F` | 全局展开 / 固定遥测面板 |
| 左键拖拽横条 / 抓手 | 自由移动停靠位置，静止后自动磁吸与记忆 |
| 双击横条 / 点击设置图标 | 打开或关闭统计与设置工作台 |
| 拖拽展开面板上/下外缘把手 | 实时调整展开面板高度（`200–720 px`） |
| 右键横条 | 快捷菜单：模式、币种、语言、主题、累计范围（周/月/全）、五方位停靠、穿透、工作台、退出 |
| 右键系统托盘图标 | 显隐横条、取消穿透、切换主题、复位位置、打开设置、退出 |
| 日历中依次点击两个日期 | 自选日历与趋势统计区间 |

---

## 代码结构

```text
build.rs                 # 编译期生成多尺寸应用图标、UAC Manifest 与 PE VERSIONINFO 元数据
src/
├── main.rs            # 单实例互斥、无边框透明窗口初始化、按停靠预设定位、分层字体加载
├── app.rs             # 应用主状态、事件唤醒循环、窗口尺寸与位置自适应、单测冒烟套件
├── collector.rs       # 采集调度（CC Switch / 原生模式）与严格按时间序的实时速率追踪
├── config.rs          # 配置持久化、模型单价表与别名解析、有效成本与缓存节省计算、汇率同步
├── http_client.rs     # 跨平台 HTTPS 客户端（Linux ureq + rustls / Windows 原生 WinHTTP）
├── pricing_catalog.rs # models.dev / OpenRouter 在线定价目录异步拉取、合并与本地缓存
├── detector.rs        # 本机已安装 AI 编程工具探测
├── history_cache.rs   # T-1 历史累计加速缓存与单价变更内存重算
├── cache_db.rs        # 嵌入式 SQLite WAL 增量缓存库（O(1) 脏键点写、事务原子性与旧 JSON 自动迁移）
├── native_scanner.rs  # Claude(+subagents) / Antigravity / Codex(.zst) / Copilot 增量解析与归档
├── project_stats.rs   # 跨工具项目路径、Antigravity 摘要 DB 与会话标题索引、项目排行榜引擎
├── range_stats.rs     # 日历热力、多周期区间摘要与消耗全景下钻（RangeBreakdown）引擎、CSV/TSV 导出
├── stats.rs           # 今日按工具/按模型切片、PeriodBaseCache 多周期模型矩阵聚合
├── theme.rs           # Modern Glass & Cyber OSD 视觉令牌与高光/准星表面装饰
├── i18n.rs            # 简体中文 / English 国际化字典与时间相对格式
├── tray.rs            # 跨平台系统托盘（Linux KSNI D-Bus / Windows 托盘）、动态悬停汇总
├── hotkeys.rs         # 全局热键后台监听（Linux X11 grab_key / Windows RegisterHotKey）
├── watcher.rs         # 基于原生文件事件（Linux inotify / Windows ReadDirectoryChangesW）的多目录监听
├── win_fx.rs          # 跨平台单实例锁与 Unix IPC、屏幕工作区几何、边缘磁吸与穿透控制
├── autostart.rs       # 跨平台开机自启动（Linux XDG Autostart / Windows HKCU Run）与路径自愈
└── ui/
    ├── mod.rs          # 通用表格单元格、胶囊标签、切换开关、Sparkline 波形绘图组件
    ├── hud_bar.rs      # 紧凑/多模式 HUD 横条排版与预算进度条渲染
    ├── flyout.rs       # 可固定/可调高三合一遥测弹出面板（多周期矩阵 / 今日分布 / 实时请求流）
    ├── context_menu.rs # HUD 右键上下文快捷菜单
    ├── analytics.rs    # 工作台：日历与趋势页、消耗全景下钻页、项目与会话排行页
    ├── pricing_page.rs # 工作台：汇率与模型定价目录行内编辑/新增页
    └── studio.rs       # 工作台框架、侧边栏导航、外观与驱动设置页
```

---

## 致谢

感谢开源项目 [CC Switch](https://github.com/farion1231/cc-switch)。本项目支持直接只读读取其数据库中的请求日志与每日归档表，并在此基础上补充了全量原生扫描与双视图工作台。

## 许可证

[MIT License](LICENSE)。欢迎提交 Issue 与 Pull Request。
