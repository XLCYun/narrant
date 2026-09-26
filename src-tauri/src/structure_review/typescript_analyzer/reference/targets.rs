//! 定义位置必须命中真实声明或经服务确认的别名链；运行时身份不含安装目录。
#[path = "aliases.rs"]
mod aliases;
use super::super::{
    failure,
    lsp::{CheckedLocation, DefinitionEvidence, TypeScriptLsp},
    syntax::{Declaration, Document, Occurrence, OccurrenceKind},
    Result,
};
use crate::structure_review::code_graph::{
    AnalysisOptions, CodeCategory, ExternalSymbol, NodeId, ReferenceTarget, SymbolScope,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// 已解析的声明身份保留原生类别，用于校验值声明和定义查询的歧义。
#[derive(Clone)]
struct Target {
    declaration: Declaration,
    runtime: bool,
}

/// 缓存以受控文件为键；成员集合永不被上下文加载修改。
pub(super) struct Resolver {
    documents: BTreeMap<PathBuf, Document>,
    membership: BTreeSet<NodeId>,
    alias_cache: BTreeMap<(PathBuf, PathBuf, u32, Vec<String>), bool>,
    pub external_symbols: BTreeMap<String, ExternalSymbol>,
}
impl Resolver {
    /// 接收输出文档的所有权，稍后只为实际定义目标补充上下文。
    pub fn new(documents: BTreeMap<PathBuf, Document>, membership: BTreeSet<NodeId>) -> Self {
        Self {
            documents,
            membership,
            alias_cache: BTreeMap::new(),
            external_symbols: BTreeMap::new(),
        }
    }

    /// 以每个项目的 definition 导航结果为准；不可信或不匹配证据保持不确定。
    pub fn resolve(
        &mut self,
        session: &mut TypeScriptLsp,
        file: &Path,
        occurrence: &Occurrence,
        project: &Path,
    ) -> Result<Option<BTreeSet<ReferenceTarget>>> {
        if occurrence.dynamic
            || occurrence.type_only
                && matches!(
                    occurrence.kind,
                    OccurrenceKind::Call | OccurrenceKind::Construct | OccurrenceKind::JsxComponent
                )
        {
            return Ok(None);
        }
        let source = &self.documents[file].source;
        if occurrence.span.start > occurrence.selection.start
            || occurrence.selection.end > occurrence.span.end
            || source.get(occurrence.selection.start as usize..occurrence.selection.end as usize)
                != Some(&occurrence.text)
        {
            return Err(failure("引用位置与原始源码不一致"));
        }
        if matches!(
            occurrence.kind,
            OccurrenceKind::Call
                | OccurrenceKind::Construct
                | OccurrenceKind::JsxComponent
                | OccurrenceKind::Reference
        ) && (session.runtime_type_error(file, occurrence.selection.start, project)
            || session.runtime_type_error(file, occurrence.span.start, project))
        {
            return Ok(None);
        }
        if matches!(
            occurrence.kind,
            OccurrenceKind::Call
                | OccurrenceKind::Construct
                | OccurrenceKind::JsxComponent
                | OccurrenceKind::Reference
        ) && self.type_only_binding(
            session,
            file,
            occurrence.root_binding,
            &occurrence.members,
            project,
        )? {
            return Ok(None);
        }
        let definitions = session.definitions(file, occurrence.position, project)?;
        if definitions.is_empty() {
            return Ok(None);
        }
        let mut targets = BTreeMap::new();
        let mut uncertain = false;
        for definition in definitions {
            let DefinitionEvidence::Checked(location) = definition else {
                uncertain = true;
                continue;
            };
            let Some(resolved) = self.follow(
                session,
                &location,
                project,
                occurrence.kind,
                &mut BTreeSet::new(),
            )?
            else {
                uncertain = true;
                continue;
            };
            for target in resolved {
                targets.insert(target.declaration.node.data().id.clone(), target);
            }
        }
        if targets.is_empty() {
            return Ok(None);
        }
        // 已观察到函数值但证据冲突时保留未知引用，普通数据读取仍不扩展成图边。
        let unknown_reference = || {
            (occurrence.kind == OccurrenceKind::Reference
                && targets
                    .values()
                    .any(|target| accepts(target, occurrence.kind)))
            .then(|| {
                BTreeSet::from([ReferenceTarget::Unresolved {
                    name: occurrence.semantic_text.clone(),
                }])
            })
        };
        if uncertain {
            return Ok(unknown_reference());
        }
        if !targets
            .values()
            .all(|target| accepts(target, occurrence.kind))
        {
            return Ok(unknown_reference());
        }
        if targets.len() > 1 {
            // 多个源码片段只对合并类型/命名空间有确定集合意义，调用不可选取第一个。
            let first = targets.values().next().unwrap().declaration.node.data();
            if !targets.values().all(|t| {
                matches!(
                    t.declaration.node.category(),
                    CodeCategory::Type | CodeCategory::Container
                ) && t.declaration.node.data().name == first.name
                    && t.declaration.node.data().native_kind == first.native_kind
            }) || matches!(
                occurrence.kind,
                OccurrenceKind::Call
                    | OccurrenceKind::Construct
                    | OccurrenceKind::JsxComponent
                    | OccurrenceKind::Reference
            ) {
                return Ok(unknown_reference());
            }
        }
        Ok(Some(
            targets
                .into_values()
                .map(|target| self.reference(target))
                .collect(),
        ))
    }

    /// 别名续查限定深度并检测回环；只沿精确导入/导出名或 const 直接绑定继续。
    fn follow(
        &mut self,
        session: &mut TypeScriptLsp,
        location: &CheckedLocation,
        project: &Path,
        kind: OccurrenceKind,
        visited: &mut BTreeSet<(PathBuf, u32)>,
    ) -> Result<Option<Vec<Target>>> {
        if visited.len() >= 32 || !visited.insert((location.path.clone(), location.span.start_byte))
        {
            return Ok(None);
        }
        if !self.load(session, location)? {
            return Ok(None);
        }
        let document = &self.documents[&location.path];
        let alias = document
            .aliases
            .iter()
            .find(|alias| {
                alias.selection.is_some_and(|selection| {
                    selection.start <= location.span.start_byte
                        && location.span.end_byte <= selection.end
                })
            })
            .cloned();
        let declaration = declaration_at(document, location).cloned();
        let value_alias = if alias.is_none()
            && matches!(
                kind,
                OccurrenceKind::Call
                    | OccurrenceKind::Construct
                    | OccurrenceKind::Reference
                    | OccurrenceKind::JsxComponent
            ) {
            document
                .value_aliases
                .iter()
                .find(|alias| {
                    alias.binding.start <= location.span.start_byte
                        && location.span.end_byte <= alias.binding.end
                })
                .cloned()
        } else {
            None
        };
        let runtime_value = matches!(
            kind,
            OccurrenceKind::Call
                | OccurrenceKind::Construct
                | OccurrenceKind::Reference
                | OccurrenceKind::JsxComponent
        );
        // 导入/导出别名使用独立语法位置，不需要伪造一个图节点作为所有者。
        let query = if let Some(alias) = alias {
            let selection = alias.selection.expect("已按别名查询位置筛选");
            let position = document
                .source
                .position(selection.start as usize)
                .map_err(failure)?;
            if runtime_value
                && self.alias_route(
                    session,
                    &location.path,
                    &alias,
                    &[],
                    project,
                    &mut BTreeSet::new(),
                )?
            {
                return Ok(None);
            }
            Some((position, selection.start, alias.type_only))
        } else if let Some(alias) = value_alias {
            let position = document
                .source
                .position(alias.selection.start as usize)
                .map_err(failure)?;
            if runtime_value
                && self.type_only_binding(
                    session,
                    &location.path,
                    alias.root_binding,
                    &[],
                    project,
                )?
            {
                return Ok(None);
            }
            Some((position, alias.selection.start, false))
        } else {
            None
        };
        if let Some((position, offset, type_only)) = query {
            if session.project().source_paths.contains(&location.path) {
                session.check_runtime_type_usage(&location.path, project)?;
            }
            if runtime_value
                && (type_only || session.runtime_type_error(&location.path, offset, project))
            {
                return Ok(None);
            }
            let definitions = session.definitions(&location.path, position, project)?;
            if definitions.is_empty() {
                return Ok(None);
            }
            let mut result = Vec::new();
            for definition in definitions {
                let DefinitionEvidence::Checked(next) = definition else {
                    return Ok(None);
                };
                let Some(targets) =
                    self.follow(session, &next, project, kind, &mut visited.clone())?
                else {
                    return Ok(None);
                };
                result.extend(targets);
            }
            return Ok(Some(result));
        }
        Ok(declaration.map(|declaration| {
            vec![Target {
                runtime: !session.project().source_paths.contains(&location.path),
                declaration,
            }]
        }))
    }

    /// 复用 LSP 已校验的位置和共享源码；仅首次遇到目标文件时解析其声明。
    fn load(&mut self, session: &TypeScriptLsp, location: &CheckedLocation) -> Result<bool> {
        let project = session.project();
        let path = &location.path;
        if project.metadata.contains(path) {
            return Ok(false);
        }
        if !self.documents.contains_key(path) {
            let runtime = !project.source_paths.contains(path);
            let root = if runtime {
                &project.toolchain.runtime_root
            } else {
                &project.workspace_root
            };
            let mut document = Document::parse(
                root,
                path.clone(),
                project.source(path)?,
                AnalysisOptions {
                    include_tests: true,
                },
            )?;
            if runtime {
                for declaration in &mut document.declarations {
                    let data = super::super::syntax::data_mut(&mut declaration.node);
                    data.id.0 = runtime_id(&data.id.0);
                    for child in &mut data.contains {
                        child.0 = runtime_id(&child.0);
                    }
                    if let Some(parent) = &mut declaration.parent {
                        parent.0 = runtime_id(&parent.0);
                    }
                }
                for occurrence in &mut document.occurrences {
                    occurrence.owner.0 = runtime_id(&occurrence.owner.0);
                }
            }
            self.documents.insert(path.clone(), document);
        }
        Ok(true)
    }

    /// 只有最初选集中的声明可成为 Node；外部说明不参与身份或差异。
    fn reference(&mut self, target: Target) -> ReferenceTarget {
        let data = target.declaration.node.data();
        if self.membership.contains(&data.id) {
            return ReferenceTarget::Node(data.id.clone());
        }
        self.external_symbols.insert(
            data.id.0.clone(),
            ExternalSymbol {
                name: data
                    .name
                    .clone()
                    .unwrap_or_else(|| data.native_kind.clone()),
                scope: if target.runtime {
                    SymbolScope::Dependency
                } else {
                    SymbolScope::Repository
                },
            },
        );
        ReferenceTarget::External {
            symbol: data.id.0.clone(),
        }
    }
}

/// 运行时的相对包路径仍保留；安装目录和临时侧路径永不进入身份。
fn runtime_id(id: &str) -> String {
    id.replacen(":repo:", ":runtime:", 1)
}

/// 明确声明类别与语义意图对应；裸调用签名和 import alias 永远不是运行时实现。
fn accepts(target: &Target, kind: OccurrenceKind) -> bool {
    let declaration = &target.declaration;
    let native = declaration.node.data().native_kind.as_str();
    let callable = declaration.callable_value.is_some()
        || declaration.node.category() == CodeCategory::Callable
            && !matches!(native, "getter" | "setter")
            && (declaration.parts.iter().any(|p| p.body.is_some())
                || target.runtime
                || declaration.ambient_function);
    match kind {
        OccurrenceKind::Call | OccurrenceKind::Reference => callable,
        OccurrenceKind::Construct => native == "class" || native == "constructor" && callable,
        OccurrenceKind::JsxComponent => callable || native == "class",
        OccurrenceKind::Extends | OccurrenceKind::Implements | OccurrenceKind::TypeUse => {
            declaration.node.category() == CodeCategory::Type
        }
    }
}

/// 只接受名字范围或直接函数初始化器；匿名回调内部位置不能借用外层声明身份。
fn declaration_at<'a>(
    document: &'a Document,
    location: &CheckedLocation,
) -> Option<&'a Declaration> {
    if location.span.start_byte >= location.span.end_byte {
        return None;
    }
    document
        .declared_at(location.span.start_byte)
        .filter(|d| {
            d.span.start <= location.span.start_byte
                && location.span.end_byte <= d.span.end
                && d.selections
                    .iter()
                    .any(|s| s.start <= location.span.start_byte && location.span.end_byte <= s.end)
        })
        .or_else(|| {
            document.declarations.iter().find(|d| {
                d.callable_value.is_some_and(|span| {
                    span.start == location.span.start_byte && span.end == location.span.end_byte
                }) || matches!(d.node.data().native_kind.as_str(), "function" | "class")
                    && d.node.data().name.is_none()
                    && d.parts.iter().any(|p| {
                        p.selection.is_empty()
                            && p.span.start == location.span.start_byte
                            && location.span.end_byte <= p.span.end
                    })
            })
        })
}

#[cfg(test)]
#[path = "../tests/targets.rs"]
mod tests;
