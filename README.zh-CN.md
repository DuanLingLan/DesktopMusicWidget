<div align="center">

# DesktopMusicWidget

**面向 Windows 10/11 的轻量级常驻桌面音乐小部件。**

它待在桌面层——在壁纸和桌面图标之上、普通窗口之下——用一张小玻璃卡片播放一个
文件夹里的音乐，你可以随手点它，而不用离开手头正在做的事。

[![CI](https://github.com/DuanLingLan/DesktopMusicWidget/actions/workflows/ci.yml/badge.svg)](https://github.com/DuanLingLan/DesktopMusicWidget/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-lightgrey)

[English](README.md)

</div>

---

<!-- Screenshots: drop yours in as assets/screenshot-card.png, screenshot-menu.png
     and screenshot-settings.png, then uncomment the block below.
     Suggested capture: Win+Shift+S over the card, the right-click menu, and the
     settings window.

<p align="center">
  <img src="assets/screenshot-card.png" width="420" alt="The widget on the desktop">
  <img src="assets/screenshot-menu.png" width="320" alt="The right-click menu">
  <br>
  <img src="assets/screenshot-settings.png" width="520" alt="The settings window">
</p>
-->

## 功能

- **属于桌面层，而不是一个窗口。** 不挡路：从不抢焦点，从不出现在 Alt-Tab 中，
  也从不遮住你的工作内容。
- **Wallpaper Engine 是可选的。** 装了它，卡片会叠在正在运行的壁纸之上；没装，
  卡片则改为锚定到资源管理器的桌面层。两种情况都开箱即用。
- **无需安装程序，无需运行时。** 一个自包含的 `.exe`，不需要任何配置步骤。
- **首次运行向导。** 选好音乐文件夹；程序会在你确认之前告诉你找到了多少首曲目。
- **可控的扫描范围。** 可以包含或跳过子文件夹，按名称或路径排除文件夹，并选择
  哪些扩展名算作音乐。
- **完整的右键菜单**，卡片和托盘图标上都有：播放控制、播放模式、音乐目录、
  扫描选项、开机自启动、启动时自动播放、窗口层级、显示器。
- **真正的设置窗口**，可调尺寸、位置、不透明度、音量、字体、窗口层级、显示器、
  界面语言和快捷键——即时生效，并保存为可读的 TOML 文件。（卡片配色是唯一需要
  直接编辑配置文件的项目。）
- **可选的开机自启动和启动时自动播放。** 只写 HKCU，因此永远不会请求管理员权限。
- **双语界面。** 中文和英文，跟随 Windows 界面语言。
- **小巧安静。** 约 3 MB，使用 CPU 辅助的 Direct2D 渲染，私有内存约 2 MB，而且
  只在内容真正变化时才重绘。

## 下载与运行

1. 从 [Releases](../../releases) 下载 `DesktopMusicWidget-v1.0.0-windows-x64-msvc.zip`
   （或者 `-gnu` 版本——它们是同一个程序用两种工具链构建出来的）。
2. 解压到任意位置。没有安装程序。
3. 运行 `desktop-music-widget.exe`。
4. 首次运行向导会询问你的音乐文件夹。选好它们，确认曲目数量，播放就开始了。

可选：右键点击卡片（或托盘图标）→ **开机自启动**。

要校验你的下载，请用同一个 Release 中的 `SHA256SUMS.txt` 核对 zip。

## 使用

### 卡片

| 操作 | 作用 |
|---|---|
| 悬停 | 显示播放控制按钮 |
| 点击 ‹ / ▶ / › | 上一首 / 播放-暂停 / 下一首 |
| 点击或拖动进度条 | 跳转 |
| 鼠标滚轮 | 音量（仅在卡片有焦点时；其他情况请用快捷键） |
| 右键 | 完整菜单 |
| `Ctrl` + 拖动，或 **移动窗口（拖动）** | 重新摆放卡片；位置会被保存 |
| 左键点击托盘图标 | 播放 / 暂停 |
| 双击托盘图标 | 设置 |

### 右键菜单

```
播放 / 暂停
下一首
上一首
─────────────────────────────
播放模式            ▸  顺序播放 / 随机播放 / 单曲循环
─────────────────────────────
音乐目录            ▸  添加文件夹… / 替换为… / 打开第一个目录 / 清空
扫描选项            ▸  包含子文件夹 / 排除文件夹… / 重新扫描音乐库
─────────────────────────────
开机自启动
启动时自动播放
显示控制按钮        ▸  悬停时显示 / 始终显示
窗口层级            ▸  桌面层（推荐） / 置顶显示
显示器              ▸  显示器 1 / 显示器 2 / …
─────────────────────────────
移动窗口（拖动）
设置…
打开配置文件 / 重新加载配置
─────────────────────────────
关于 DesktopMusicWidget
退出
```

菜单里没有用来移除文件夹的条目，因为设置窗口里有一个带移除按钮的列表；菜单只放
你经常改动的东西。

### 设置窗口

| 分区 | 内容 |
|---|---|
| 音乐目录 | 文件夹列表，可添加 / 移除 / 打开 |
| 扫描范围 | 包含子文件夹、排除列表，以及一个**实时的"已找到 N 首音乐文件"**计数器，随你的改动即时更新 |
| 启动与播放 | 开机自启动、启动时自动播放、播放模式、音量 |
| 外观与位置 | 宽度、高度、圆角、不透明度、控制按钮显示时机、窗口层级、显示器、字体 |
| 通用 | 界面语言、全局快捷键及其方案 |
| 底部按钮栏 | 恢复默认、打开配置文件、取消 / 应用 / 确定 |

在你按下 **应用** 或 **确定** 之前不会写入任何内容，所以 **取消** 是真的丢弃改动。
唯一的例外是语言选择器，它会立即生效，好让你看到自己选了什么。

### 键盘

| 快捷键 | 作用 |
|---|---|
| `Ctrl`+`Alt`+`Space` | 播放 / 暂停 |
| `Ctrl`+`Alt`+`←` / `→` | 上一首 / 下一首 |
| `Ctrl`+`Alt`+`↑` / `↓` | 提高 / 降低音量 |

`Ctrl+Alt+方向键` 在部分机器上被 Intel 显卡驱动占用，因此小部件会自动改用加上
`Shift` 的组合重试。你可以在设置里切换方案或关闭快捷键；如果仍有组合无法注册，
设置窗口会报告是哪一组。

## Wallpaper Engine

Wallpaper Engine **不是必需的**。卡片不是 Wallpaper Engine 壁纸，也不依赖它。

- **在 Wallpaper Engine 运行时**，卡片直接叠在它的窗口之上，这让层级关系是确定的。
- **不运行它时**，没有可以叠靠的对象，因此单纯的"放在 Z 序最底部"可能最终落到
  桌面图标层*下面*。所以小部件改为把自身锚定到资源管理器的桌面层
  （`Progman` / `WorkerW`），也就是绘制壁纸和图标的那个窗口。

如果卡片不出现，请运行 `diag.exe`——它会打印 Z 序，告诉你选中的是哪个锚点，并
说明卡片在桌面层之上还是之下。

## 配置

设置窗口会写入 `%APPDATA%\DesktopMusicWidget\config.toml`。所有内容也都在
[`config.example.toml`](config.example.toml) 中有说明，右键菜单里的
**重新加载配置** 可以让你手动编辑后无需重启就生效。

一个最小配置：

```toml
library = ["C:\\Users\\You\\Music"]
play_mode = "shuffle"
autoplay = true
autostart = false
```

常用字段：

| 字段 | 默认值 | 说明 |
|---|---|---|
| `config_version` | `2` | 架构版本。别去动它：缺失该值会被当作 v1 并迁移。 |
| `library` | `[]` | 绝对路径。留空会触发首次运行向导。 |
| `scan_recursive` | `true` | 是否同时扫描子文件夹。 |
| `exclude_dirs` | `[]` | 只写名字时匹配任何同名文件夹；写路径时匹配该文件夹及其下的全部内容。 |
| `extensions` | mp3, flac, wav, m4a, m4b, aac, ogg, oga, aiff | 必须与音频后端能够解码的格式一致。 |
| `play_mode` | `"shuffle"` | `sequential`、`shuffle`、`repeat-one`。 |
| `volume` | `70` | 0–100，会经过一条幂曲线映射。 |
| `autoplay` | `true` | 关闭后仍会加载音乐库，但保持暂停。 |
| `autostart` | `false` | 写入 `HKCU\...\Run`；移动 exe 后会自动修正。 |
| `hotkeys_enabled` | `true` | 是否注册全局快捷键。 |
| `hotkey_preset` | `"ctrl-alt"` | 也可以是 `ctrl-shift-alt`。 |
| `mode` | `"bottom"` | `bottom`（桌面层）或 `topmost`。 |
| `monitor` | `-1` | `-1` = 主显示器，否则是从 0 开始的序号。 |
| `anchor` | `"top-center"` | 也可以是 `top-left`、`top-right`、`free`（绝对坐标）。 |
| `offset_x` / `offset_y` | 0 / 24 | 相对锚点的偏移，单位为逻辑像素。`anchor = "free"` 时为绝对坐标。 |
| `width` / `height` | 340 / 96 | 逻辑像素，宽度 200–900，高度 64–400。 |
| `corner_radius` | `14.0` | 0 到 `height / 2`。 |
| `card_opacity` | `0.78` | 0.0–1.0。 |
| `show_controls` | `"hover"` | 也可以是 `always`。 |
| `font_family` | `"Segoe UI"` | 任意已安装的字体。 |
| `language` | `"auto"` | `auto`、`zh-CN`、`en`。 |
| `[theme]` | 见下文 | 六个 `#rrggbb` / `#rrggbbaa` 颜色。 |

`[theme]` 的键是 `background`、`art_bg`、`title`、`subtitle`、`bar_bg` 和
`bar_fg`；无法解析的值会静默回退到默认值，而不是渲染出一张看不见的卡片。
这些是设置窗口唯一没有暴露的项目——在这里改完后用**重新加载配置**生效。

错误的值永远不会让程序停下来：它们会被钳制或重置为默认值，记录到 `widget.log`，
并写回文件，因此该文件始终描述正在运行的状态。损坏的文件会被备份为
`config.toml.bak` 并替换为默认值。

### 便携模式

如果 exe 旁边放着一个 `config.toml`，就会使用那个文件，且不会向 `%APPDATA%`
写入任何内容。适合放在 U 盘或自包含文件夹里。

## 命令行

```
desktop-music-widget.exe [OPTIONS]

  -h, --help              Show help
  -V, --version           Show version
      --config-dir <DIR>  Use DIR for config and logs
      --portable          Keep config next to the exe
      --lang <LANG>       auto | zh-CN | en
      --autoplay, --no-autoplay
                          Override autoplay for this run
      --install-autostart Add to the logon entry
      --remove-autostart  Remove the logon entry
      --open-config       Open config.toml and exit
      --open-settings     Open the settings window at startup
      --reset-config      Back up and rewrite defaults
      --dump-config       Print the effective config
      --count-tracks [--dir <DIR>]
                          Count tracks and exit
      --play <FILE>...    Play these files instead of the library
  -v, --verbose           Also log to the console
```

`--count-tracks` 和 `--dump-config` 不需要窗口，CI 任务正是用它们在无头 runner 上
覆盖扫描器和配置代码。

## 疑难解答

**Windows SmartScreen 警告我。** 这些二进制文件没有代码签名（对一个业余项目来说
证书要花钱）。选择 **更多信息** → **仍要运行**。如果愿意，可以先校验 SHA256。

**卡片看不见。** 运行 `diag.exe`。它会打印 Z 序、卡片在桌面层之上还是之下，以及
选中的是哪个锚点。最常见的原因是上面有一个全屏窗口，或者 `mode` 设为 `topmost`
而其上还有别的窗口。

**没有声音。** 在 `widget.log` 里查找 "audio device not available"。程序在启动时会
对默认输出设备重试约 20 秒，因为它可能先于音频服务启动。卡片会显示
"没有可用的音频设备"，托盘也会弹出一次气泡提示。

**什么都不播放 /"这个位置没有音乐文件"。** 打开 设置 → 扫描范围，看看找到曲目的
计数器。通常是文件夹选错了、扩展名不匹配，或者排除规则命中得比预期更多（只写名字
会排除*所有*同名文件夹）。

**它找到了我的一部分音乐，但不全。** 音频后端不支持 Opus 和 WMA，因此它们有意
不在 `extensions` 中；加进去只会造成静默失败而不是播放。`.aiff`、`.m4b` 和 `.aac`
是支持的。

**某首曲目显示的是文件名而不是标题。** 那个文件没有标题标签——WAV 通常是这种
情况，从未打过标签的抓轨文件也是如此。

**文字模糊。** 程序通过内嵌清单实现了 per-monitor DPI 感知，所以这不应该发生。
如果确实发生，请检查你是否在通过兼容性模式的 DPI 覆盖来运行它（右键 exe →
属性 → 兼容性）。

**鼠标滚轮不能调音量。** Windows 把滚轮消息发送给拥有焦点的窗口，而卡片刻意从不
获取焦点。请使用快捷键、托盘菜单或设置里的滑块。

**设置窗口里的中文 / 日文显示不正常。** 如果系统区域设置比较特殊，只有拉丁文本会
用错字体回退；歌曲标题始终使用系统区域设置。请附上你的区域设置开一个 issue。

**登录时启动了两个副本。** 你可能同时注册了本版本和一个更早的版本。设置里有一个
**移除旧版自启动项** 按钮，用来清除改名前的那个条目。

**卸载。** 从托盘菜单退出，删除文件夹；如果启用过，请先取消勾选 **开机自启动**，
或者删除 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 下的
`DesktopMusicWidget` 值。设置和日志位于 `%APPDATA%\DesktopMusicWidget`。

## 从源码构建

需要：[Rust](https://rustup.rs) 1.80 或更新版本，以及与你所选工具链匹配的 Windows
链接器。

```powershell
git clone https://github.com/DuanLingLan/DesktopMusicWidget
cd DesktopMusicWidget

# GNU toolchain (matches what the maintainer develops against; needs MinGW's
# gcc and windres on PATH)
rustup target add x86_64-pc-windows-gnu
cargo build --release

# or MSVC (needs Visual Studio Build Tools)
cargo build --release --target x86_64-pc-windows-msvc
```

exe 会出现在 `target/release/`，与 `diag.exe` 放在一起。

内嵌清单、图标和版本资源由 `build.rs` 生成。`assets/icon.ico` 由
`assets/make_icon.ps1`（纯算术，不依赖绘图库）生成并已提交，因此只有在你改动字形
时才需要运行那个脚本。如果没有可用的资源编译器，构建会带着警告继续——你只会失去
图标和主题化控件，其他都不受影响。

## 验证构建

```powershell
cargo test --all-targets          # 95 unit tests: config, migration, scanner, menus, i18n
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

# End-to-end smoke test against the release binary: generates silent WAV
# fixtures, then checks decoding, the first-run wizard, the settings window and
# the card's Z order.
powershell -File tools\verify.ps1
```

`tools/verify.ps1` 需要完整语言的 PowerShell（它用到的 `System.IO` 流类型在受限
语言模式下不可用）。

## 工作原理

一个 Win32 进程、一个 UI 线程和三个后台线程。

```
main.rs          arguments, single-instance mutex, message loop
  app.rs         window procedure, layout, Z order, tray, actions
    menu.rs      the shared right-click menu (TPM_RETURNCMD, so no WM_COMMAND)
    settings.rs  the settings window (standard Win32 controls)
    dialogs.rs   folder picker, Explorer, message boxes
  render.rs      Direct2D + DirectWrite onto a WIC bitmap, blitted to a
                 layered window with UpdateLayeredWindow
  theme.rs       colours and fonts, parsed from config
  library.rs     background folder walk + preview counter
  audio.rs       rodio player on its own thread
  config.rs      schema, validation, v1 migration, atomic saves
  autostart.rs   HKCU Run entry
  hotkeys.rs     global hotkeys
  i18n.rs        the string catalog
  log.rs         rotating log file and a panic hook
```

如果你要改动什么，以下这些点值得知道：

- **Z 序。** 卡片是 `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`，并且
  `WM_MOUSEACTIVATE` 返回 `MA_NOACTIVATE`，所以它永远不会获得焦点。选择
  Wallpaper Engine 还是桌面层，见 `app::desktop_anchor_kind`。
- **Alpha。** `UpdateLayeredWindow` 需要预乘的 BGRA，因此 Direct2D 目标以
  `D2D1_ALPHA_MODE_PREMULTIPLIED` 创建，并且此后绝不再让 GDI 接触那个 DIB。
  `SourceConstantAlpha` 必须保持 255——卡片的不透明度改为烘焙进画刷里。
- **播放很快开始。** 扫描一个大型音乐库需要数秒，因此遍历会先送出一个*起始*专辑
  （随机选取的一个顶层文件夹，只需毫秒级时间），随后再送出音乐库的其余部分。
- **曲目交接。** 自动切歌是通过把 `Player::len()` 与记账不变量 `current + appended`
  比较来检测的，而不是观察长度下降——因为补充会先执行，会掩盖那次变化。
- **托盘回调** 刻意停留在旧的 `NOTIFYICON_VERSION` 上，此时回调报告的是
  `WM_LBUTTONUP` / `WM_RBUTTONUP`。在版本 4 下它报告的是 `NIN_SELECT` /
  `WM_CONTEXTMENU`，经典的双击就丢失了。

欢迎贡献——见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 已知限制

- 仅支持 Windows 10/11 x64。渲染和窗口代码自始至终都是 Win32 和 Direct2D；其他
  平台需要另做一个前端。
- 无法播放 Opus 和 WMA：音频后端没有它们的解码器。
- 二进制文件未签名，因此首次运行会有 SmartScreen 警告。
- 歌词、流媒体服务、播放列表和均衡器明确不在范围内——这是一个轻量的文件夹播放器，
  不是音乐库管理器。

## 许可证

[MIT](LICENSE)。播放使用 [rodio](https://github.com/RustAudio/rodio)，标签读取使用
[lofty](https://github.com/Serial-ATA/lofty-rs)，Win32 API 使用
[windows](https://github.com/microsoft/windows-rs) crate。
