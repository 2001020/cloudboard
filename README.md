# Cloudboard 27

面向课堂大屏与触控一体机的电子白板。Rust 编写，支持 Windows 与 macOS。

- 书写：笔压、荧光笔、虚线/点线、局部或整笔擦除、图形识别、文字、图片、选择/复制/改色
- PDF：分块多级渲染，打开与滚动都很快，可在 PDF 上直接批注
- PowerPoint（.pptx）与 Word（.docx）：内置渲染器，无需安装 Office/WPS；可在页面上批注；PPT 支持全屏放映，动画简化为淡入、飞入、淡出和强调
- 录制：录制整个窗口画面、麦克风和电脑声音，保存为 MP4，分辨率可在设置中选择
- 教具：直尺、45° 与 30°/60° 三角板、量角器、圆规
- 计时器：倒计时、秒表、计次、到时提醒、全屏大字
- 连接手机/平板：电脑开启热点（Windows）或使用同一局域网，手机扫码后在网页上书写，笔迹实时同步到大屏；手机上可双指放大书写，可拍照或从相册选多张图片发送到白板
- 在线更新：从本仓库的 GitHub Releases 获取，经签名和 SHA-256 校验
- 兼容 CloudBoard 26 的 `.cloudboard` 工程文件（只读兼容，保存时转为新格式）

## 仓库结构

```
crates/
  cb-core      文档模型、命令式撤销、擦除、图形识别、教具几何、计时器、文件格式、设置（纯逻辑）
  cb-render    tiny-skia 绘制、cosmic-text 文字（中文回退到系统字体）、图标、主题、墨迹与教具绘制
  cb-pdf       页面渲染线程（PDFium 或 Office 渲染器）、分块多级缓存
  cb-office    PPTX/DOCX 解析、排版与绘制（母版/版式/主题、样式继承、表格、图片、动画步骤）
  cb-record    录制：恒定帧率、缩放、音频重采样与混音；编码器由平台提供
  cb-ui        应用状态机：输入、工具、底栏/弹窗/对话框/面板、画板渲染、启动窗
  cb-update    GitHub Releases 更新客户端、cb-release 发布工具
  cb-remote    局域网 HTTP/WebSocket 服务与内嵌手机网页
  cb-shell     winit + softbuffer 窗口外壳（两个平台共用）
platforms/
  windows          Windows 可执行文件：WM_POINTER 笔输入、WinRT 热点、文件关联、对话框、Media Foundation 录制
  windows-updater  更新安装程序（等待退出 → 替换 → 失败回滚 → 重启）
  macos            macOS 可执行文件：对话框、提示音、.app 替换更新、AVAssetWriter 录制、ScreenCaptureKit 电脑声音
web/remote/    手机/平板网页客户端（单文件，无外部依赖）
packaging/     Inno Setup 安装脚本、macOS Info.plist
scripts/       打包脚本、端到端测试脚本
legacy/cpp/    CloudBoard 26（C++）旧代码，仅供参考
docs/          旧版功能盘点、发布说明
```

## 构建

需要 Rust stable（1.80+）。

```bash
cargo test --workspace
```

Windows（PowerShell，产出便携 zip 和安装包，需 Inno Setup 6 才会生成安装包）：

```bash
pwsh scripts/package-windows.ps1
```

macOS（产出通用二进制 `Cloudboard.app` 的 zip）：

```bash
scripts/package-macos.sh
```

运行时需要 PDFium 动态库（打包脚本会自动下载 [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries)）。开发时可用环境变量 `CLOUDBOARD_PDFIUM` 指向库文件。

## 发布与在线更新

1. 生成签名密钥（只需一次）：

   ```bash
   cargo run -p cb-update --bin cb-release -- keygen
   ```

   把输出的 `public` 写入 `crates/cb-update/release-public-key.txt` 并提交；把 `private` 保存为仓库 Secret `UPDATE_SIGNING_KEY`。在公钥文件为空时，客户端只依赖 HTTPS 和 SHA-256 校验。

2. 修改 `Cargo.toml` 中的 `version`，更新 `docs/release-notes.md`，然后推送标签：

   ```bash
   git tag v27.0.1
   ```

   ```bash
   git push origin v27.0.1
   ```

3. `release.yml` 会构建 Windows x64/arm64 与 macOS 通用版，生成并签名 `latest.json`，发布 GitHub Release。客户端启动时（可在设置中关闭）和点击“检查更新”时读取最新发布。

国内网络访问 GitHub 不稳定时，可在设置文件中填写 `update_mirror`（下载地址前缀）。

## 开发与测试

- 界面快照：`cargo test -p cb-ui` 会在 `target/snapshots/` 生成 100%–250% 缩放下的界面截图。
- PDF 滚动基准：

  ```bash
  CLOUDBOARD_PDFIUM=/path/libpdfium.so taskset -c 0,1 cargo run --release -p cb-ui --example pdf_scroll_bench -- 200 1920 1080 1.0
  ```

  在 2 核限制下，200 页 PDF、1080p：每帧 UI 线程耗时中位数约 1.2 ms，P99 约 4.4 ms；4K/200%：中位数约 4.6 ms，P99 约 17.8 ms。

- 手机端到端：先运行 `cargo run -p cb-ui --example remote_host -- url.txt 40`，再用 Playwright 运行 `node scripts/remote-e2e.js url.txt 输出目录`（书写、撤销、双指放大后书写、上传三张图片）。
- Office 渲染：`cargo run --release -p cb-office --example render -- 文件.pptx 输出目录 1280` 把每页渲染为 PNG；测试文档由 `crates/cb-office/tests/data/make_*.py`（python-pptx / python-docx）生成，可与 LibreOffice 的输出对照。

## 已知限制

- macOS 没有开启热点的公开接口，手机需与电脑处于同一局域网。
- macOS 上数位板笔压暂未接入（winit 尚不提供），Windows 支持笔压与笔尾橡皮。
- 未配置 Apple 开发者证书时，macOS 版为临时签名，首次打开需右键选择“打开”。
- 界面使用系统字体（Windows 读取系统消息字体，通常为微软雅黑；macOS 为系统字体/苹方），不再内置字体文件。
- Office 文档为“基本还原”：不支持旧的 .ppt/.doc（请另存为 .pptx/.docx）、图表（显示占位框）、EMF/WMF 矢量图、艺术字特效、3D、视频音频；文字排版与 Office 可能略有差异。
- 录制：Windows N 版需安装“媒体功能包”；macOS 录制电脑声音需要 macOS 13 及以上并在“系统设置 > 隐私与安全性 > 屏幕录制”中允许，否则只录麦克风。
