use std::path::Path;

fn main() {
    // 内置插件后端是独立 crate 的编译产物，主程序用 include_bytes! 把它嵌进来。
    // 若插件还没构建，编译会以一个晦涩的 "file not found" 失败。
    // 这里提前检查并给出明确指引。
    let plugin_bin = Path::new("../plugins/sticky-summarizer/target/release/sticky-summarizer.exe");
    println!("cargo:rerun-if-changed=../plugins/sticky-summarizer/target/release/sticky-summarizer.exe");
    println!("cargo:rerun-if-changed=../plugins/sticky-summarizer/src/main.rs");

    if !plugin_bin.exists() {
        panic!(
            "\n\n\
             ╔══════════════════════════════════════════════════════════════╗\n\
             ║  内置插件后端尚未构建，无法继续编译主程序。                  ║\n\
             ╠══════════════════════════════════════════════════════════════╣\n\
             ║  请先执行（在项目根目录）：                                  ║\n\
             ║      npm run plugin:build                                    ║\n\
             ║  或手动：                                                    ║\n\
             ║      cd plugins/sticky-summarizer && cargo build --release   ║\n\
             ╚══════════════════════════════════════════════════════════════╝\n\n"
        );
    }

    tauri_build::build()
}
