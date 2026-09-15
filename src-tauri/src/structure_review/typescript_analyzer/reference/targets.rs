//! 定义位置必须命中真实声明或经服务确认的别名链；运行时身份不含安装目录。
#[path = "aliases.rs"]
mod aliases;
use super::super::{
    failure, identity,
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

/// 已解析的声明身份保留原生类别，用于拒绝类型签名和运行时目标歧义。
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

    /// 每个项目保留完整目标集合；任意不可信或不匹配证据都会使本次关系不确定。
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
        ) && self.type_only_binding(session, file, occurrence, project)?
        {
            return Ok(None);
        }
        let definitions = session.definitions(file, occurrence.position, project)?;
        if definitions.is_empty() {
            return Ok(None);
        }
        let mut targets = BTreeMap::new();
        let mut modules = BTreeSet::new();
        let mut uncertain = false;
        for definition in definitions {
            let DefinitionEvidence::Checked(location) = definition else {
                uncertain = true;
                continue;
            };
            if matches!(
                occurrence.kind,
                OccurrenceKind::Import | OccurrenceKind::Reexport
            ) && location.span.start_byte == 0
            {
                if let Some(target) = self.module(session, &location)? {
                    modules.insert(target);
                    continue;
                }
            }
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
        let member_names = targets
            .values()
            .filter_map(member_name)
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        if occurrence.kind == OccurrenceKind::Call && !member_names.is_empty() {
            for definition in session.type_definitions(file, occurrence.position, project)? {
                let DefinitionEvidence::Checked(location) = definition else {
                    return Ok(None);
                };
                if let Some(additional) = self.follow(
                    session,
                    &location,
                    project,
                    occurrence.kind,
                    &mut BTreeSet::new(),
                )? {
                    for target in additional {
                        let data = target.declaration.node.data();
                        // 同名成员可能采用方法或函数值字段；返回类型本身不是成员，不能混入候选。
                        if member_name(&target).is_some_and(|name| member_names.contains(name)) {
                            targets.insert(data.id.clone(), target);
                        }
                    }
                } else {
                    // 无法映射的补充位置也不能被悄悄丢弃后恢复确定性。
                    uncertain = true;
                }
            }
        }
        if targets.is_empty() {
            return Ok((!uncertain && !modules.is_empty()).then_some(modules));
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
        for target in targets.into_values() {
            modules.insert(self.reference(target));
        }
        Ok(Some(modules))
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
        if location.unverified
            || visited.len() >= 32
            || !visited.insert((location.path.clone(), location.span.start_byte))
        {
            return Ok(None);
        }
        if !self.load(session, location)? {
            return Ok(None);
        }
        let document = &self.documents[&location.path];
        let alias = document
            .occurrences
            .iter()
            .find(|o| {
                o.import_alias
                    && o.selection.start <= location.span.start_byte
                    && location.span.end_byte <= o.selection.end
            })
            .cloned();
        let declaration = declaration_at(document, location).cloned();
        let alias = alias.or_else(|| {
            declaration.as_ref().and_then(|d| {
                if !matches!(
                    kind,
                    OccurrenceKind::Call
                        | OccurrenceKind::Construct
                        | OccurrenceKind::Reference
                        | OccurrenceKind::JsxComponent
                ) {
                    return None;
                }
                let selection = d.alias_selection?;
                document
                    .occurrences
                    .iter()
                    .find(|o| {
                        o.owner == d.node.data().id
                            && o.kind == OccurrenceKind::Reference
                            && !o.type_only
                            && !o.dynamic
                            && o.selection == selection
                    })
                    .cloned()
            })
        });
        if let Some(alias) = alias {
            if matches!(
                kind,
                OccurrenceKind::Call
                    | OccurrenceKind::Construct
                    | OccurrenceKind::Reference
                    | OccurrenceKind::JsxComponent
            ) && self.type_only_binding(session, &location.path, &alias, project)?
            {
                return Ok(None);
            }
            if session.project().sources.contains_key(&location.path) {
                session.collect_notes(&location.path, project)?;
            }
            if (alias.type_only
                || session.runtime_type_error(&location.path, alias.selection.start, project))
                && matches!(
                    kind,
                    OccurrenceKind::Call
                        | OccurrenceKind::Construct
                        | OccurrenceKind::Reference
                        | OccurrenceKind::JsxComponent
                )
            {
                return Ok(None);
            }
            let definitions = session.definitions(&location.path, alias.position, project)?;
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
                runtime: !session.project().sources.contains_key(&location.path),
                declaration,
            }]
        }))
    }

    /// JSON 元数据不进入 Oxc；标准库和固定 Node 闭包使用独立稳定命名空间。
    fn load(&mut self, session: &TypeScriptLsp, location: &CheckedLocation) -> Result<bool> {
        let project = session.project();
        let checked =
            project.path_from_uri(&crate::lsp::file_uri(&location.path).map_err(failure)?)?;
        if checked != location.path || project.source_uri(&checked)? != location.uri {
            return Err(failure("定义 URI 与受控源码身份不一致"));
        }
        if project.metadata.contains(&checked) {
            return Ok(false);
        }
        if !self.documents.contains_key(&checked) {
            let (root, path, runtime) = if let Some(snapshot) = project.sources.get(&checked) {
                (project.workspace_root.clone(), snapshot.path.clone(), false)
            } else {
                let path = url::Url::parse(&location.uri)
                    .map_err(failure)?
                    .to_file_path()
                    .map_err(|_| failure("运行时 URI 无效"))?;
                if !["typescript/lib", "@types/node", "undici-types"]
                    .iter()
                    .any(|p| path.starts_with(project.toolchain.runtime_root.join(p)))
                {
                    return Err(failure("定义不属于固定运行时源码"));
                }
                (project.toolchain.runtime_root.clone(), path, true)
            };
            let mut document = Document::parse(
                &root,
                path,
                project.source(&checked)?,
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
            self.documents.insert(checked.clone(), document);
        }
        let document = &self.documents[&checked];
        if document.uri != location.uri
            || super::super::text::byte_offset(&document.source, &location.span.range.start)?
                != location.span.start_byte
            || super::super::text::byte_offset(&document.source, &location.span.range.end)?
                != location.span.end_byte
        {
            return Err(failure("定义位置不属于原始文档快照"));
        }
        Ok(true)
    }

    /// 文件级模块没有虚构图节点；导入文件本身仍有稳定的外部模块身份。
    fn module(
        &mut self,
        session: &TypeScriptLsp,
        location: &CheckedLocation,
    ) -> Result<Option<ReferenceTarget>> {
        if !self.load(session, location)? {
            return Ok(None);
        }
        let document = &self.documents[&location.path];
        let runtime = !session.project().sources.contains_key(&location.path);
        let root = if runtime {
            &session.project().toolchain.runtime_root
        } else {
            &session.project().workspace_root
        };
        let mut symbol = identity::file_scope(root, &document.path, &document.language.0)?;
        if runtime {
            symbol = runtime_id(&symbol);
        }
        symbol.push_str("::module");
        let path = document.path.strip_prefix(root).map_err(failure)?.display();
        self.external_symbols.insert(
            symbol.clone(),
            ExternalSymbol {
                name: format!("{path}（模块）"),
                scope: if runtime {
                    SymbolScope::Dependency
                } else {
                    SymbolScope::Repository
                },
            },
        );
        Ok(Some(ReferenceTarget::External { symbol }))
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

/// 同名成员证据跨越方法、字段和结构声明；是否可调用在完整候选集合上统一检查。
fn member_name(target: &Target) -> Option<&str> {
    let data = target.declaration.node.data();
    matches!(
        data.native_kind.as_str(),
        "method"
            | "static_method"
            | "object_method"
            | "field"
            | "static_field"
            | "object_property"
            | "property"
            | "parameter_property"
            | "accessor_property"
            | "getter"
            | "setter"
    )
    .then_some(data.name.as_deref())
    .flatten()
}

/// 明确声明类别与语义意图对应；裸调用签名和 import alias 永远不是运行时实现。
fn accepts(target: &Target, kind: OccurrenceKind) -> bool {
    let declaration = &target.declaration;
    let native = declaration.node.data().native_kind.as_str();
    let callable = declaration.callable_value.is_some()
        || declaration.node.category() == CodeCategory::Callable
            && !matches!(
                native,
                "call_signature" | "construct_signature" | "getter" | "setter"
            )
            && (declaration.parts.iter().any(|p| p.body.is_some())
                || target.runtime
                || declaration.ambient_function);
    match kind {
        OccurrenceKind::Call | OccurrenceKind::Reference => callable,
        OccurrenceKind::Construct => native == "class" || native == "constructor" && callable,
        OccurrenceKind::JsxComponent => callable || native == "class",
        OccurrenceKind::Write => {
            declaration.node.category() == CodeCategory::Value || native == "setter"
        }
        OccurrenceKind::Extends | OccurrenceKind::Implements | OccurrenceKind::TypeUse => {
            declaration.node.category() == CodeCategory::Type
        }
        OccurrenceKind::Import | OccurrenceKind::Reexport => {
            declaration.node.category() != CodeCategory::Extension
        }
    }
}

/// 只接受名字范围或直接函数初始化器；匿名回调内部位置不能借用外层声明身份。
fn declaration_at<'a>(
    document: &'a Document,
    location: &CheckedLocation,
) -> Option<&'a Declaration> {
    if location.unverified || location.span.start_byte >= location.span.end_byte {
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
