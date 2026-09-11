DesktopMusicWidget
==================

A lightweight, always-on-desktop music widget for Windows 10/11 (64-bit).
轻量的桌面音乐悬浮窗，常驻桌面壁纸之上、普通窗口之下。

QUICK START / 快速开始
----------------------

1. Double-click desktop-music-widget.exe. / 双击 desktop-music-widget.exe。
2. On first run it asks for your music folders. Pick them and confirm.
   首次启动会询问音乐目录，选好并确认。
3. It starts playing. Right-click the card (or the tray icon) for everything
   else - play mode, folders, scan scope, settings, autostart, exit.
   选好即开始播放。右键卡片或托盘图标可打开完整菜单（播放模式、目录、扫描
   范围、设置、开机自启、退出）。
4. Left-click the tray icon to play/pause, double-click it for Settings.
   单击托盘图标播放/暂停，双击打开设置。

Wallpaper Engine is NOT required. / 不需要 Wallpaper Engine。

FILES / 文件说明
----------------

  desktop-music-widget.exe  the widget itself / 主程序
  diag.exe                  diagnostics: z-order, desktop anchor, monitors
                            (run it when the card cannot be seen)
                            诊断工具：层级、桌面锚点、显示器
  config.example.toml       every configuration option, documented
                            全部配置项及说明
  README.md                 full documentation / 完整文档
  LICENSE                   MIT

WHERE THINGS ARE STORED / 配置文件位置
--------------------------------------

  %APPDATA%\DesktopMusicWidget\config.toml   settings / 设置
  %APPDATA%\DesktopMusicWidget\widget.log    log (attach it to bug reports)
                                            日志（报告问题时请附上）

Portable mode: if a config.toml sits next to the exe, that one is used and
nothing is written to %APPDATA%.
便携模式：若 exe 同目录存在 config.toml，则使用该文件，不写入 %APPDATA%。

NOTES / 说明
------------

* Autostart uses a per-user registry entry (HKCU), so it never asks for admin
  rights. / 开机自启写 HKCU，无需管理员权限。
* The binary is not code-signed. If SmartScreen warns, choose "More info" then
  "Run anyway". / 程序未签名，SmartScreen 提示时选"更多信息"→"仍要运行"。
* Global hotkeys default to Ctrl+Alt+Space / Ctrl+Alt+arrows and can be changed
  or disabled in Settings. / 全局快捷键默认 Ctrl+Alt+空格与方向键，可在设置中
  修改或关闭。

MIT licensed. Source: https://github.com/DuanLingLan/DesktopMusicWidget
