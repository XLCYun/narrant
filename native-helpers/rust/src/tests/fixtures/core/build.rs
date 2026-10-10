/// 用于验证原生分析行为的 fixture 函数。
fn main() {
    std::fs::write("build-script-executed", "forbidden").unwrap();
}
