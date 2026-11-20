//! 自动更新：从 GitHub Releases 读取最新版本，下载当前平台的安装包，
//! 用 Release 里的 `SHA256SUMS.txt` 校验后替换正在运行的程序，再按需重启。
//!
//! 安装包命名（由 `.github/workflows/release.yml` 生成）：
//! - macOS arm64：`MDEasy-<版本>-macos-arm64.zip`（内含 `MDEasy.app`）
//! - Windows x64：`MDEasy-<版本>-windows-x64.zip`（内含 `mdeasy.exe`）
//! - Linux x64：`MDEasy-<版本>-linux-x64.tar.gz`（内含 `mdeasy`）

use std::{
    io::Read as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result, anyhow, bail};
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{actions::Quit, workspace::Workspace};

pub const REPO: &str = "ifconfiger/MDEasy";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CHECKSUMS: &str = "SHA256SUMS.txt";
/// 自动检查的最小间隔
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

// ---------- Release 信息 ----------

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

impl Release {
    pub fn version(&self) -> Option<semver::Version> {
        semver::Version::parse(self.tag_name.trim_start_matches('v')).ok()
    }

    pub fn is_newer_than(&self, current: &str) -> bool {
        match (self.version(), semver::Version::parse(current)) {
            (Some(latest), Ok(current)) => latest > current,
            _ => false,
        }
    }

    pub fn display_version(&self) -> String {
        self.version().map(|v| v.to_string()).unwrap_or_else(|| self.tag_name.clone())
    }

    fn asset_named(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    pub fn platform_asset(&self) -> Option<&Asset> {
        let suffix = platform_suffix()?;
        self.assets.iter().find(|a| a.name.starts_with("MDEasy-") && a.name.ends_with(suffix))
    }
}

/// 当前平台对应的安装包后缀；未提供安装包的平台返回 `None`。
pub fn platform_suffix() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("-macos-arm64.zip")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("-windows-x64.zip")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("-linux-x64.tar.gz")
    } else {
        None
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(60))
        .user_agent(&format!("MDEasy/{CURRENT_VERSION}"))
        // 尊重 HTTPS_PROXY / ALL_PROXY 等代理环境变量
        .try_proxy_from_env(true)
        .build()
}

/// 读取最新的正式版本。还没有任何 Release 时返回 `None`。
pub fn fetch_latest() -> Result<Option<Release>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    match agent().get(&url).set("Accept", "application/vnd.github+json").call() {
        Ok(resp) => Ok(Some(resp.into_json().context("无法解析版本信息")?)),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(code, _)) => bail!("GitHub 返回 HTTP {code}"),
        Err(err) => Err(anyhow!(err).context("无法连接 GitHub")),
    }
}

fn download(url: &str, dest: &Path) -> Result<()> {
    let resp = agent().get(url).call().with_context(|| format!("下载失败：{url}"))?;
    let mut file = std::fs::File::create(dest)?;
    std::io::copy(&mut resp.into_reader(), &mut file).context("下载中断")?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// 从 `sha256sum` 格式的文本里找出某个文件的校验和。
fn find_checksum<'a>(sums: &'a str, file_name: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == file_name).then_some(hash)
    })
}

// ---------- 安装 ----------

/// 要被替换的路径：macOS 上是 `MDEasy.app`，其他平台是可执行文件本身。
pub fn install_target() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("找不到当前程序路径")?;
    #[cfg(not(windows))]
    let exe = exe.canonicalize().unwrap_or(exe);
    let in_target_dir = exe.ancestors().any(|p| {
        p.file_name().is_some_and(|n| n == "debug" || n == "release")
            && p.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "target")
    });
    if in_target_dir {
        bail!("当前是开发构建（{}），不支持自动安装", exe.display());
    }
    if cfg!(target_os = "macos") {
        let bundle = exe
            .ancestors()
            .nth(3)
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
            .ok_or_else(|| anyhow!("当前不是从 MDEasy.app 运行，无法自动安装"))?;
        Ok(bundle.to_path_buf())
    } else {
        Ok(exe)
    }
}

/// 下载并安装，返回安装后的路径（用于重启）。在后台线程调用。
pub fn download_and_install(release: &Release) -> Result<PathBuf> {
    let target = install_target()?;
    install_release(release, &target)?;
    Ok(target)
}

fn install_release(release: &Release, target: &Path) -> Result<()> {
    let asset = release.platform_asset().ok_or_else(|| anyhow!("这个版本没有提供当前平台的安装包"))?;
    let sums_asset = release.asset_named(CHECKSUMS).ok_or_else(|| anyhow!("版本缺少 {CHECKSUMS}"))?;
    let parent = target.parent().ok_or_else(|| anyhow!("无效的安装路径"))?;

    // 临时目录放在安装目录旁边，保证最后一步可以原子 rename
    let staging = parent.join(format!(".mdeasy-update-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("没有写入 {} 的权限", parent.display()))?;
    let result = install_from(&staging, asset, sums_asset, target);
    _ = std::fs::remove_dir_all(&staging);
    result
}

fn install_from(staging: &Path, asset: &Asset, sums_asset: &Asset, target: &Path) -> Result<()> {
    let archive = staging.join(&asset.name);
    download(&asset.browser_download_url, &archive)?;

    let sums_path = staging.join(CHECKSUMS);
    download(&sums_asset.browser_download_url, &sums_path)?;
    let sums = std::fs::read_to_string(&sums_path)?;
    let expected = find_checksum(&sums, &asset.name)
        .ok_or_else(|| anyhow!("{CHECKSUMS} 里没有 {}", asset.name))?;
    let actual = sha256_file(&archive)?;
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("安装包校验失败（SHA-256 不一致），已放弃安装");
    }

    let extracted = staging.join("extracted");
    std::fs::create_dir_all(&extracted)?;
    extract(&archive, &extracted)?;

    let wanted = if cfg!(target_os = "macos") {
        "MDEasy.app"
    } else if cfg!(windows) {
        "mdeasy.exe"
    } else {
        "mdeasy"
    };
    let new_item = find_entry(&extracted, wanted, 3)
        .ok_or_else(|| anyhow!("安装包里没有找到 {wanted}"))?;
    replace(&new_item, target)
}

fn extract(archive: &Path, dest: &Path) -> Result<()> {
    // 三个平台都自带解压工具：macOS 用 ditto（保留 .app 的权限和签名），
    // Windows 10+ / Linux 用 tar（Windows 的 bsdtar 也能解 zip）
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/ditto");
        c.arg("-x").arg("-k").arg(archive).arg(dest);
        c
    } else {
        let mut c = Command::new("tar");
        c.arg("-xf").arg(archive).arg("-C").arg(dest);
        c
    };
    hide_console(&mut cmd);
    let status = cmd.stdin(Stdio::null()).status().context("无法运行解压命令")?;
    if !status.success() {
        bail!("解压安装包失败（{status}）");
    }
    Ok(())
}

fn find_entry(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    if let Some(e) = entries.iter().find(|e| e.file_name() == name) {
        return Some(e.path());
    }
    if depth == 0 {
        return None;
    }
    entries
        .iter()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .find_map(|e| find_entry(&e.path(), name, depth - 1))
}

/// 用新版本替换 `target`。正在运行的程序文件可以被改名，所以先把旧的挪开再放入新的。
fn replace(new_item: &Path, target: &Path) -> Result<()> {
    let parent = target.parent().unwrap_or(Path::new("."));
    let backup = if cfg!(windows) {
        target.with_extension("exe.old")
    } else {
        parent.join(format!(".mdeasy-old-{}", std::process::id()))
    };

    #[cfg(unix)]
    if !cfg!(target_os = "macos") {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(new_item, std::fs::Permissions::from_mode(0o755))?;
    }

    remove_path(&backup);
    std::fs::rename(target, &backup)
        .with_context(|| format!("无法替换 {}（权限不足？）", target.display()))?;
    if let Err(err) = std::fs::rename(new_item, target) {
        _ = std::fs::rename(&backup, target);
        return Err(anyhow!(err).context(format!("无法写入 {}", target.display())));
    }
    // Windows 上正在运行的 exe 删不掉，留到下次启动时清理
    if !cfg!(windows) {
        remove_path(&backup);
    }
    if cfg!(target_os = "macos") {
        _ = Command::new("/usr/bin/xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(target)
            .stderr(Stdio::null())
            .status();
    }
    Ok(())
}

fn remove_path(path: &Path) {
    if path.is_dir() {
        _ = std::fs::remove_dir_all(path);
    } else {
        _ = std::fs::remove_file(path);
    }
}

/// 清理上次更新留下的旧文件（Windows 的 `mdeasy.exe.old`）。
pub fn cleanup_previous_update() {
    if cfg!(windows)
        && let Ok(exe) = std::env::current_exe()
    {
        _ = std::fs::remove_file(exe.with_extension("exe.old"));
    }
}

/// 启动一个小进程：等当前进程退出后重新打开 MDEasy。
/// 如果 60 秒内没有退出（例如用户在“未保存”对话框里取消了），就不再重启。
pub fn schedule_relaunch(target: &Path) -> Result<()> {
    let pid = std::process::id();
    #[cfg(unix)]
    {
        let launch = if cfg!(target_os = "macos") { r#"open "$1""# } else { r#"exec "$1""# };
        let script = format!(
            "i=0; while kill -0 {pid} 2>/dev/null && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done; \
             kill -0 {pid} 2>/dev/null || {launch}"
        );
        Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .arg("mdeasy-relaunch")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    }
    #[cfg(windows)]
    {
        let path = target.display().to_string().replace('\'', "''");
        let script = format!(
            "$p = Get-Process -Id {pid} -ErrorAction SilentlyContinue; \
             if ($p) {{ $null = $p.WaitForExit(60000) }}; \
             if (-not (Get-Process -Id {pid} -ErrorAction SilentlyContinue)) {{ Start-Process -FilePath '{path}' }}"
        );
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script]);
        hide_console(&mut cmd);
        cmd.spawn()?;
    }
    Ok(())
}

fn hide_console(_cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        _cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

// ---------- 状态（上次检查时间、跳过的版本） ----------

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct UpdateState {
    last_check: u64,
    skipped_version: Option<String>,
}

impl UpdateState {
    fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("MDEasy").join("update.json"))
    }

    fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        if let Some(path) = Self::path() {
            if let Some(dir) = path.parent() {
                _ = std::fs::create_dir_all(dir);
            }
            _ = std::fs::write(path, serde_json::to_vec_pretty(self).unwrap_or_default());
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------- 界面流程 ----------

/// 启动时自动检查（发布版、距上次检查超过一天）。设置 `MDEASY_NO_UPDATE_CHECK=1` 可关闭。
pub fn check_on_startup(window: &mut Window, cx: &mut Context<Workspace>) {
    if cfg!(debug_assertions) || std::env::var_os("MDEASY_NO_UPDATE_CHECK").is_some() {
        return;
    }
    let state = UpdateState::load();
    if now_secs().saturating_sub(state.last_check) < CHECK_INTERVAL.as_secs() {
        return;
    }
    check(false, window, cx);
}

/// 检查更新。`manual` 为 true 时（菜单触发）无论结果都给出反馈。
pub fn check(manual: bool, window: &mut Window, cx: &mut Context<Workspace>) {
    if manual {
        window.push_notification(Notification::info("正在检查更新…"), cx);
    }
    let task = cx.background_spawn(async move { fetch_latest() });
    cx.spawn_in(window, async move |this, cx| {
        let result = task.await;
        let mut state = UpdateState::load();
        state.last_check = now_secs();
        state.save();
        _ = this.update_in(cx, |_, window, cx| match result {
            Ok(Some(release))
                if release.is_newer_than(CURRENT_VERSION)
                    && (manual || state.skipped_version.as_deref() != Some(&release.tag_name)) =>
            {
                offer(release, window, cx)
            }
            Ok(_) => {
                if manual {
                    window.push_notification(
                        Notification::success(format!("已是最新版本（{CURRENT_VERSION}）")),
                        cx,
                    );
                }
            }
            Err(err) => {
                if manual {
                    window.push_notification(
                        Notification::warning(format!("{err:#}")).title("检查更新失败"),
                        cx,
                    );
                } else {
                    eprintln!("检查更新失败：{err:#}");
                }
            }
        });
    })
    .detach();
}

fn offer(release: Release, window: &mut Window, cx: &mut Context<Workspace>) {
    let version = release.display_version();
    let unsupported = match (release.platform_asset(), install_target()) {
        (None, _) => Some("这个版本没有提供当前平台的安装包。".to_string()),
        (_, Err(err)) => Some(format!("{err}。")),
        _ => None,
    };
    let mut notes: String = release.body.clone().unwrap_or_default().trim().chars().take(800).collect();
    if release.body.as_ref().is_some_and(|b| b.trim().chars().count() > 800) {
        notes.push('…');
    }
    let mut detail = format!("当前版本 {CURRENT_VERSION}。");
    if let Some(reason) = &unsupported {
        detail.push_str(reason);
        detail.push_str("可以到下载页手动更新。");
    }
    if !notes.is_empty() {
        detail.push_str("\n\n");
        detail.push_str(&notes);
    }
    let can_install = unsupported.is_none();
    let first = if can_install { "下载并安装" } else { "打开下载页" };
    let answer = window.prompt(
        PromptLevel::Info,
        &format!("发现新版本 MDEasy {version}"),
        Some(&detail),
        &[first, "稍后", "跳过这个版本"],
        cx,
    );
    cx.spawn_in(window, async move |this, cx| {
        let Ok(answer) = answer.await else { return };
        match answer {
            0 if can_install => {
                _ = this.update_in(cx, |_, window, cx| install(release, window, cx));
            }
            0 => {
                _ = this.update(cx, |_, cx| cx.open_url(&release.html_url));
            }
            2 => {
                let mut state = UpdateState::load();
                state.skipped_version = Some(release.tag_name.clone());
                state.save();
            }
            _ => {}
        }
    })
    .detach();
}

fn install(release: Release, window: &mut Window, cx: &mut Context<Workspace>) {
    let version = release.display_version();
    window.push_notification(
        Notification::info(format!("正在下载 MDEasy {version}，完成后会提示你重启。")).title("更新"),
        cx,
    );
    let release_bg = release.clone();
    let task = cx.background_spawn(async move { download_and_install(&release_bg) });
    cx.spawn_in(window, async move |this, cx| {
        let result = task.await;
        _ = this.update_in(cx, |_, window, cx| match result {
            Ok(target) => {
                let answer = window.prompt(
                    PromptLevel::Info,
                    &format!("MDEasy {version} 已安装"),
                    Some("重启 MDEasy 后生效。"),
                    &["立即重启", "稍后"],
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    if answer.await != Ok(0) {
                        return;
                    }
                    if let Err(err) = schedule_relaunch(&target) {
                        eprintln!("无法安排重启：{err:#}");
                    }
                    _ = this.update_in(cx, |_, window, cx| {
                        window.dispatch_action(Box::new(Quit), cx)
                    });
                })
                .detach();
            }
            Err(err) => {
                let answer = window.prompt(
                    PromptLevel::Warning,
                    "更新失败",
                    Some(&format!("{err:#}\n\n可以到下载页手动更新。")),
                    &["打开下载页", "关闭"],
                    cx,
                );
                let url = release.html_url.clone();
                cx.spawn_in(window, async move |this, cx| {
                    if answer.await == Ok(0) {
                        _ = this.update(cx, |_, cx| cx.open_url(&url));
                    }
                })
                .detach();
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    // 不用 `super::*`：gpui 的 `test` 宏会遮住标准库的 `#[test]`
    use super::{Asset, Release, find_checksum, platform_suffix, sha256_file};
    use std::path::{Path, PathBuf};

    fn release(tag: &str, assets: &[&str]) -> Release {
        Release {
            tag_name: tag.into(),
            body: None,
            html_url: String::new(),
            assets: assets
                .iter()
                .map(|n| Asset { name: (*n).into(), browser_download_url: String::new() })
                .collect(),
        }
    }

    #[test]
    fn compares_versions() {
        assert!(release("v0.2.0", &[]).is_newer_than("0.1.0"));
        assert!(release("0.1.1", &[]).is_newer_than("0.1.0"));
        assert!(!release("v0.1.0", &[]).is_newer_than("0.1.0"));
        assert!(!release("v0.0.9", &[]).is_newer_than("0.1.0"));
        assert!(!release("nightly", &[]).is_newer_than("0.1.0"));
        // 预发布版本低于同号正式版
        assert!(!release("v0.1.0-beta.1", &[]).is_newer_than("0.1.0"));
    }

    #[test]
    fn picks_platform_asset() {
        let r = release(
            "v0.2.0",
            &[
                "MDEasy-0.2.0-macos-arm64.zip",
                "MDEasy-0.2.0-windows-x64.zip",
                "MDEasy-0.2.0-linux-x64.tar.gz",
                "SHA256SUMS.txt",
            ],
        );
        match platform_suffix() {
            Some(suffix) => assert!(r.platform_asset().unwrap().name.ends_with(suffix)),
            None => assert!(r.platform_asset().is_none()),
        }
    }

    #[test]
    fn parses_checksums() {
        let sums = "abc123  MDEasy-0.2.0-linux-x64.tar.gz\nDEF456 *MDEasy-0.2.0-windows-x64.zip\n";
        assert_eq!(find_checksum(sums, "MDEasy-0.2.0-linux-x64.tar.gz"), Some("abc123"));
        assert_eq!(find_checksum(sums, "MDEasy-0.2.0-windows-x64.zip"), Some("DEF456"));
        assert_eq!(find_checksum(sums, "missing.zip"), None);
    }

    #[test]
    fn hashes_file() {
        let path = std::env::temp_dir().join(format!("mdeasy-sha-test-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        _ = std::fs::remove_file(path);
    }

    /// 极简 HTTP 服务：按路径返回 `root` 下的文件，用来模拟 GitHub Release 下载。
    fn serve(root: PathBuf) -> String {
        use std::io::{BufRead as _, BufReader, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap() == 0 || h == "\r\n" {
                        break;
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/');
                let mut stream = stream;
                match std::fs::read(root.join(path)) {
                    Ok(body) => {
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        _ = stream.write_all(head.as_bytes());
                        _ = stream.write_all(&body);
                    }
                    Err(_) => {
                        _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            }
        });
        format!("http://{addr}")
    }

    /// 造一个“新版本”安装包，返回包名。
    fn make_package(dir: &Path, version: &str) -> String {
        let src = dir.join("src");
        let suffix = platform_suffix().unwrap();
        let name = format!("MDEasy-{version}{suffix}");
        let run = |cmd: &mut std::process::Command| assert!(cmd.status().unwrap().success());
        if cfg!(target_os = "macos") {
            let app = src.join("MDEasy.app/Contents/MacOS");
            std::fs::create_dir_all(&app).unwrap();
            std::fs::write(app.join("MDEasy"), version).unwrap();
            run(std::process::Command::new("ditto")
                .args(["-c", "-k", "--keepParent"])
                .arg(src.join("MDEasy.app"))
                .arg(dir.join(&name)));
        } else {
            let pkg = src.join(format!("MDEasy-{version}"));
            std::fs::create_dir_all(&pkg).unwrap();
            std::fs::write(pkg.join("mdeasy"), version).unwrap();
            run(std::process::Command::new("tar")
                .arg("-C")
                .arg(&src)
                .arg("-czf")
                .arg(dir.join(&name))
                .arg(format!("MDEasy-{version}")));
        }
        name
    }

    fn installed_content(target: &Path) -> String {
        let file = if cfg!(target_os = "macos") {
            target.join("Contents/MacOS/MDEasy")
        } else {
            target.to_path_buf()
        };
        std::fs::read_to_string(file).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn installs_release_end_to_end() {
        let root = std::env::temp_dir().join(format!("mdeasy-update-e2e-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&root);
        let server_dir = root.join("server");
        std::fs::create_dir_all(&server_dir).unwrap();
        let name = make_package(&server_dir, "9.9.9");
        let hash = sha256_file(&server_dir.join(&name)).unwrap();
        std::fs::write(server_dir.join("SHA256SUMS.txt"), format!("{hash}  {name}\n")).unwrap();
        let base = serve(server_dir.clone());

        // 旧版本
        let install_dir = root.join("Applications");
        let target = if cfg!(target_os = "macos") {
            let app = install_dir.join("MDEasy.app");
            std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
            std::fs::write(app.join("Contents/MacOS/MDEasy"), "0.1.0").unwrap();
            app
        } else {
            std::fs::create_dir_all(&install_dir).unwrap();
            let exe = install_dir.join("mdeasy");
            std::fs::write(&exe, "0.1.0").unwrap();
            exe
        };

        let asset = |n: &str| Asset { name: n.into(), browser_download_url: format!("{base}/{n}") };
        let release = Release {
            tag_name: "v9.9.9".into(),
            body: None,
            html_url: String::new(),
            assets: vec![asset(&name), asset("SHA256SUMS.txt")],
        };
        super::install_release(&release, &target).unwrap();
        assert_eq!(installed_content(&target), "9.9.9");
        // 不留下临时目录和备份
        let leftovers: Vec<_> = std::fs::read_dir(&install_dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".mdeasy"))
            .collect();
        assert!(leftovers.is_empty(), "残留：{leftovers:?}");

        // 校验和不匹配时拒绝安装，旧版本保持不变
        std::fs::write(server_dir.join("SHA256SUMS.txt"), format!("{}  {name}\n", "0".repeat(64))).unwrap();
        let err = super::install_release(&release, &target).unwrap_err();
        assert!(format!("{err:#}").contains("校验失败"), "{err:#}");
        assert_eq!(installed_content(&target), "9.9.9");

        _ = std::fs::remove_dir_all(&root);
    }
}
