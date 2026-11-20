//! Windows：把图标和版本信息嵌入 exe。其他平台什么都不做。

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/mdeasy.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/mdeasy.ico")
            .set("ProductName", "MDEasy")
            .set("FileDescription", "MDEasy - Markdown / JSON / YAML / TOML 编辑器");
        if let Err(err) = res.compile() {
            println!("cargo:warning=嵌入 Windows 图标失败：{err}");
        }
    }
}
