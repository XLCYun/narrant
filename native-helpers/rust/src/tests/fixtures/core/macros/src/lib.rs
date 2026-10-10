extern crate proc_macro;

#[proc_macro]
/// 用于验证原生分析行为的 fixture 函数。
pub fn marker(_: proc_macro::TokenStream) -> proc_macro::TokenStream {
    std::fs::write("proc-macro-executed", "forbidden").unwrap();
    "fn generated_by_proc_macro() {}".parse().unwrap()
}
