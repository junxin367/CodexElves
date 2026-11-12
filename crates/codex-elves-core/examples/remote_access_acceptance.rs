//! 只启动本机验收后台，不拉起或重启管理器、本地代理及公网隧道。
#[cfg(debug_assertions)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../temp/remote-acceptance");
    // 独立验收默认端口，与 start-remote-acceptance.ps1 和远程文档同步。
    let mut port = 47136;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--state-directory" => {
                directory = arguments
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("缺少验收目录"))?
                    .into()
            }
            "--port" => {
                port = arguments
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("缺少端口"))?
                    .parse()?
            }
            "--help" | "-h" => {
                println!(
                    "用法：remote_access_acceptance --port 47136 --state-directory <独立目录>"
                );
                return Ok(());
            }
            _ => anyhow::bail!("未知参数：{argument}"),
        }
    }
    codex_elves_core::remote_access::acceptance::run(directory, port).await
}

#[cfg(not(debug_assertions))]
fn main() {
    eprintln!("独立验收入口仅支持 debug 构建");
    std::process::exit(1);
}
