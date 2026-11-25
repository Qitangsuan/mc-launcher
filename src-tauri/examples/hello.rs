//! 最小对照程序：双击后开黑窗 10 秒再退出。
//! 用来判断「双击启动被系统杀死」是针对我们的程序，还是针对所有新编译 exe。

fn main() {
    println!("hello from mc-launcher test");
    println!("这个窗口会保持 10 秒，然后自动关闭");
    std::thread::sleep(std::time::Duration::from_secs(10));
}
