use std::collections::{BTreeMap, BTreeSet};

use super::{CodeNode, CodeRoot, InvalidGraph, NodeId, ReferenceTarget};

/// 校验所有端点和单一声明位置，再检查包含结构是否完整连接到根。
pub(super) fn validate(
    root: &CodeRoot,
    nodes: &BTreeMap<NodeId, CodeNode>,
) -> Result<(), InvalidGraph> {
    let mut parents = BTreeMap::new();
    for child in &root.contains {
        register_child(child, "虚拟根", nodes, &mut parents)?;
    }
    for node in nodes.values() {
        let data = node.data();
        for child in &data.contains {
            register_child(child, &data.id.0, nodes, &mut parents)?;
        }
        for reference in &data.references {
            if let ReferenceTarget::Node(target) = &reference.target {
                if !nodes.contains_key(target) {
                    return Err(InvalidGraph {
                        message: format!(
                            "节点 {} 引用了不存在的图内节点 {}。",
                            data.id.0, target.0
                        ),
                    });
                }
            }
        }
    }

    // 单一声明位置与根可达性共同保证无环，脱离根的环也会被拒绝。
    // 使用显式栈，避免深层嵌套声明耗尽 Rust 调用栈。
    let mut reachable = BTreeSet::new();
    let mut pending: Vec<_> = root.contains.iter().collect();
    while let Some(id) = pending.pop() {
        if !reachable.insert(id) {
            return Err(InvalidGraph {
                message: format!("节点 {} 的包含关系形成了环。", id.0),
            });
        }
        pending.extend(nodes[id].data().contains.iter());
    }
    if let Some(id) = nodes.keys().find(|id| !reachable.contains(id)) {
        return Err(InvalidGraph {
            message: format!("节点 {} 无法通过包含关系从虚拟根到达。", id.0),
        });
    }
    Ok(())
}

/// 确认子节点存在，且未在根、其它父节点或同一父节点下重复出现。
fn register_child<'a>(
    id: &'a NodeId,
    parent: &'a str,
    nodes: &BTreeMap<NodeId, CodeNode>,
    parents: &mut BTreeMap<&'a NodeId, &'a str>,
) -> Result<(), InvalidGraph> {
    if !nodes.contains_key(id) {
        return Err(InvalidGraph {
            message: format!("{parent} 包含不存在的节点 {}。", id.0),
        });
    }
    if let Some(previous) = parents.insert(id, parent) {
        return Err(InvalidGraph {
            message: format!("节点 {} 被 {previous} 和 {parent} 重复包含。", id.0),
        });
    }
    Ok(())
}
