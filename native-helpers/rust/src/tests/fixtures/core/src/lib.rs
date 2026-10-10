#[path = "store.rs"]
pub mod storage;
#[path = "store.rs"]
pub mod mirror;

use storage::helper;

/// 验证字段与方法的语义归属。
pub struct Counter { pub value: u32 }
impl Counter {
    /// 用于验证原生分析行为的 fixture 函数。
    pub fn update(&mut self) { self.value += helper(); }
}

/// 用于验证原生分析行为的 fixture 函数。
pub fn run() -> u32 { let f = || helper(); f() + 10 }
/// 用于验证原生分析行为的 fixture 函数。
pub fn function_value() -> fn() -> u32 { helper }
/// 用于验证原生分析行为的 fixture 函数。
pub fn direct_closure() { (|| helper())(); }
macro_rules! invoke { () => { helper() }; }
/// 用于验证原生分析行为的 fixture 函数。
pub fn macro_call() -> u32 { invoke!() }
#[cfg(test)]
#[path = "specs.rs"]
mod specifications;
#[cfg(any())]
/// 用于验证原生分析行为的 fixture 函数。
fn inactive() { missing_target(); }
marker_macros::marker!();
