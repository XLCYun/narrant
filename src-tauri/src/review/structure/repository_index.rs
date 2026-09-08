//! 仓库单侧的词法、显式导入及模块索引；不使用全局短名兜底。

use std::collections::{HashMap, HashSet};

use super::extraction_model::{
    ExtractedBinding, ExtractedBindingKind, ExtractedFile, ExtractedImport, ExtractedModulePath,
    ExtractedMutationMember,
};
use super::matching_model::DeclarationKey;
use super::model::{
    StructureCategory, StructureDiagnosticReason, StructureEvidenceKind, StructureRelationshipKind,
};

/// 明确绑定、歧义和未知保留为不同结果，供匹配与关系阶段共享。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BindingResolution {
    Local(DeclarationKey),
    External { package_name: String, name: String },
    Candidates(Vec<DeclarationKey>),
    Unknown(StructureDiagnosticReason),
}

/// 导入依赖连接到文件或外部包；多个物理候选不能强选一个。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ModuleResolution {
    Files(Vec<String>),
    External(String),
    Unknown(StructureDiagnosticReason),
}

/// TS 路径配置保持所属目录，避免把另一项目的别名套到当前源码。
#[derive(Clone, Debug, Default)]
struct TsConfig {
    directory: String,
    base: String,
    paths: Vec<(String, Vec<String>)>,
    valid: bool,
}

/// 每侧独立创建的索引只引用本侧提取结果；配置在构造时转换为 owned 值。
pub(crate) struct RepositoryIndex<'a> {
    files: HashMap<String, &'a ExtractedFile>,
    bindings: HashMap<String, HashMap<usize, HashMap<String, Vec<&'a ExtractedBinding>>>>,
    configs: Vec<TsConfig>,
    rust_modules: HashMap<(String, String), Vec<String>>,
    rust_locations: HashMap<String, Vec<(String, String)>>,
    rust_no_prelude: HashSet<String>,
    impl_members: HashMap<DeclarationKey, HashMap<String, Vec<DeclarationKey>>>,
    trait_members: HashSet<DeclarationKey>,
    mutated_bindings: HashSet<(String, usize, usize)>,
    unbound_mutations: HashSet<(String, String)>,
    this_mutations: HashMap<(DeclarationKey, bool), HashSet<Option<String>>>,
    rust_dependencies: HashMap<String, HashSet<String>>,
}

impl<'a> RepositoryIndex<'a> {
    /// 从完整侧文件与同侧配置建立路径索引，不访问工作区或外部依赖源码。
    pub(crate) fn new(files: &'a [ExtractedFile], config: &HashMap<String, String>) -> Self {
        let mut index = Self {
            files: files.iter().map(|file| (file.path.clone(), file)).collect(),
            bindings: HashMap::new(),
            configs: Vec::new(),
            rust_modules: HashMap::new(),
            rust_locations: HashMap::new(),
            rust_no_prelude: HashSet::new(),
            impl_members: HashMap::new(),
            trait_members: HashSet::new(),
            mutated_bindings: HashSet::new(),
            unbound_mutations: HashSet::new(),
            this_mutations: HashMap::new(),
            rust_dependencies: cargo_dependencies(config),
        };
        for file in files {
            let scopes = index.bindings.entry(file.path.clone()).or_default();
            for binding in &file.bindings {
                scopes
                    .entry(binding.scope)
                    .or_default()
                    .entry(binding.name.clone())
                    .or_default()
                    .push(binding);
            }
        }
        for file in files {
            // Rust 的字段值写入不改变名义类型的方法派发；函数指针局部值本来就不提升为确定声明。
            if file.path.ends_with(".rs") {
                continue;
            }
            for mutation in &file.mutations {
                // this 按类身份与成员空间记录；普通函数的动态this不能污染词法外层类。
                if mutation.name == "this" {
                    if !mutation.dynamic_this {
                        if let Some((owner, static_this)) = class_this_owner(file, mutation.scope) {
                            let member = match &mutation.member {
                                Some(ExtractedMutationMember::Named(name)) => Some(name.clone()),
                                Some(ExtractedMutationMember::Computed) | None => None,
                            };
                            for namespace in [false, true].into_iter().filter(|namespace| {
                                static_this.is_none_or(|is_static| is_static == *namespace)
                            }) {
                                index
                                    .this_mutations
                                    .entry((owner.clone(), namespace))
                                    .or_default()
                                    .insert(member.clone());
                            }
                        }
                    }
                    continue;
                }
                if let Some(bindings) = index.nearest_bindings(
                    &file.path,
                    mutation.scope,
                    &mutation.name,
                    Some(mutation.range.bytes.start),
                ) {
                    let identities: Vec<_> = bindings
                        .iter()
                        .map(|binding| {
                            (file.path.clone(), binding.scope, binding.range.bytes.start)
                        })
                        .collect();
                    index.mutated_bindings.extend(identities);
                } else {
                    // 不完整作用域下无法确认写入归属，同文件同名调用不能获得安全证明。
                    index
                        .unbound_mutations
                        .insert((file.path.clone(), mutation.name.clone()));
                }
            }
        }
        let mut paths: Vec<_> = config
            .keys()
            .filter(|path| basename(path) == "tsconfig.json")
            .collect();
        paths.sort();
        for path in paths {
            index
                .configs
                .push(load_tsconfig(path, config, &mut HashSet::new()));
        }
        index
            .configs
            .sort_by_key(|config| std::cmp::Reverse(config.directory.len()));
        index.build_rust_modules(config);
        for file in files {
            for (group_index, group) in file.impl_groups.iter().enumerate() {
                if !group.reliable {
                    continue;
                }
                if let BindingResolution::Local(owner) =
                    index.resolve_type(&file.path, group.scope, &group.owner)
                {
                    if group.trait_name.is_some() {
                        for (declaration_index, _) in file
                            .declarations
                            .iter()
                            .enumerate()
                            .filter(|(_, item)| item.impl_group == Some(group_index))
                        {
                            index
                                .trait_members
                                .insert((file.path.clone(), declaration_index));
                        }
                    }
                    let members = index.impl_members.entry(owner).or_default();
                    for (declaration_index, declaration) in
                        file.declarations.iter().enumerate().filter(|(_, item)| {
                            item.impl_group == Some(group_index) && item.reliable
                        })
                    {
                        members
                            .entry(declaration.name.clone())
                            .or_default()
                            .push((file.path.clone(), declaration_index));
                    }
                }
            }
        }
        index
    }

    /// 解析类型或 impl 头的显式路径；普通局部值绑定同样阻止跨作用域短名猜测。
    pub(crate) fn resolve_type(
        &self,
        path: &str,
        scope: usize,
        written: &str,
    ) -> BindingResolution {
        self.resolve(path, scope, written, None, &mut HashSet::new())
    }

    /// 发生点附带字节位置，使 Rust 的顺序局部绑定不会遮蔽其声明前的调用。
    pub(crate) fn resolve_occurrence(
        &self,
        path: &str,
        scope: usize,
        written: &str,
        offset: usize,
        kind: StructureRelationshipKind,
    ) -> BindingResolution {
        if kind == StructureRelationshipKind::Calls {
            let (head, _) = split_head(written);
            if self.binding_reassigned(path, scope, head, offset) {
                return unknown();
            }
            if let Some(bindings) = self.nearest_bindings(path, scope, head, Some(offset)) {
                if bindings.iter().any(|binding| {
                    binding.kind == ExtractedBindingKind::Import
                        && self.files[path].imports.iter().any(|import| {
                            import.local == head
                                && import.scope == binding.scope
                                && import.type_only
                        })
                }) {
                    return unknown();
                }
            }
        }
        if kind == StructureRelationshipKind::Calls
            && (written.contains('.') || written.contains("::"))
        {
            if let Some((receiver, member)) = split_member(written) {
                if let Some(binding) = self
                    .nearest_bindings(path, scope, receiver, Some(offset))
                    .and_then(|items| {
                        items.into_iter().find(|item| {
                            item.declaration.is_none() && item.kind != ExtractedBindingKind::Import
                        })
                    })
                {
                    return binding.type_name.as_deref().map_or(
                        BindingResolution::Unknown(StructureDiagnosticReason::DynamicTarget),
                        |type_name| {
                            let owner = self.resolve(
                                path,
                                binding.scope,
                                type_name,
                                None,
                                &mut HashSet::new(),
                            );
                            self.resolve_receiver_member(owner, member, false)
                        },
                    );
                }
            }
        }
        let resolved = self.resolve(path, scope, written, Some(offset), &mut HashSet::new());
        if kind == StructureRelationshipKind::Calls {
            let (head, _) = split_head(written);
            let imported = self
                .nearest_bindings(path, scope, head, Some(offset))
                .is_some_and(|bindings| {
                    bindings
                        .iter()
                        .any(|binding| binding.kind == ExtractedBindingKind::Import)
                });
            if imported
                && (self.resolution_reassigned(&resolved)
                    || self.resolution_reassigned(&self.resolve_type(path, scope, head)))
            {
                return unknown();
            }
        }
        if kind == StructureRelationshipKind::Calls
            && !path.ends_with(".rs")
            && written.contains('.')
        {
            return self.restrict_receiver(resolved, true);
        }
        resolved
    }

    /// 写入按词法绑定身份污染，而非全局同名；闭包执行顺序未知，不能只检查文本位置。
    fn binding_reassigned(&self, path: &str, scope: usize, name: &str, offset: usize) -> bool {
        self.unbound_mutations
            .contains(&(path.to_owned(), name.to_owned()))
            || self
                .nearest_bindings(path, scope, name, Some(offset))
                .is_some_and(|bindings| {
                    bindings.iter().any(|binding| {
                        self.mutated_bindings.contains(&(
                            path.to_owned(),
                            binding.scope,
                            binding.range.bytes.start,
                        ))
                    })
                })
    }

    /// 显式导入保留源模块的实时绑定，源声明被重写时导入调用同样不能继续确定绑定。
    fn resolution_reassigned(&self, resolution: &BindingResolution) -> bool {
        match resolution {
            BindingResolution::Local(key) => self.files.get(&key.0).is_some_and(|file| {
                file.bindings.iter().any(|binding| {
                    binding.declaration == Some(key.1)
                        && self.mutated_bindings.contains(&(
                            key.0.clone(),
                            binding.scope,
                            binding.range.bytes.start,
                        ))
                })
            }),
            BindingResolution::Candidates(keys) => keys
                .iter()
                .any(|key| self.resolution_reassigned(&BindingResolution::Local(key.clone()))),
            _ => false,
        }
    }

    /// 证据标签沿真实最近绑定生成，不能因远处存在同名 import 而错误标注。
    pub(crate) fn evidence_kind(
        &self,
        path: &str,
        scope: usize,
        written: &str,
        offset: usize,
    ) -> StructureEvidenceKind {
        if written.starts_with("this.")
            || written.starts_with("self.")
            || written.starts_with("Self::")
        {
            return StructureEvidenceKind::KnownReceiver;
        }
        let (head, tail) = split_head(written);
        if let Some(bindings) = self.nearest_bindings(path, scope, head, Some(offset)) {
            if bindings
                .iter()
                .all(|binding| binding.kind == ExtractedBindingKind::Import)
            {
                return StructureEvidenceKind::ExplicitImport;
            }
            if tail.is_some() && bindings.iter().all(|binding| binding.type_name.is_some()) {
                return StructureEvidenceKind::KnownReceiver;
            }
        }
        if tail.is_some() {
            StructureEvidenceKind::QualifiedName
        } else {
            StructureEvidenceKind::LexicalBinding
        }
    }

    /// 已知 this/self 的类型由调用上下文给出，成员绑定也复用同一索引规则。
    pub(crate) fn resolve_member(
        &self,
        owner: BindingResolution,
        member: &str,
    ) -> BindingResolution {
        match owner {
            BindingResolution::Local(key) => {
                let Some(file) = self.files.get(&key.0) else {
                    return unknown();
                };
                if matches!(
                    file.declarations[key.1].module_path,
                    Some(ExtractedModulePath::Unknown)
                ) {
                    return BindingResolution::Unknown(
                        StructureDiagnosticReason::ConfigurationUnavailable,
                    );
                }
                if !matches!(
                    file.declarations[key.1].native_kind.category,
                    StructureCategory::Class
                        | StructureCategory::Struct
                        | StructureCategory::Trait
                        | StructureCategory::Interface
                        | StructureCategory::Enum
                        | StructureCategory::Type
                        | StructureCategory::Namespace
                        | StructureCategory::Module
                ) {
                    return unknown();
                }
                let mut matches: Vec<_> = file
                    .declarations
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| {
                        item.parent == Some(key.1)
                            && item.name == member
                            && item.reliable
                            && item.impl_group.is_none()
                    })
                    .map(|(index, _)| (key.0.clone(), index))
                    .collect();
                if let Some(members) = self
                    .impl_members
                    .get(&key)
                    .and_then(|members| members.get(member))
                {
                    let inherent: Vec<_> = members
                        .iter()
                        .filter(|key| !self.trait_members.contains(*key))
                        .cloned()
                        .collect();
                    if inherent.is_empty() && matches.is_empty() {
                        // 缺少调用作用域的 trait 可见性/派发证据时，即使仅一个 impl 也只列候选。
                        let mut candidates = members.clone();
                        candidates.sort();
                        candidates.dedup();
                        return BindingResolution::Candidates(candidates);
                    }
                    matches.extend(inherent);
                }
                if file.path.ends_with(".rs")
                    && file.declarations[key.1].native_kind.category == StructureCategory::Module
                {
                    let module = &file.declarations[key.1];
                    if let ModuleResolution::Files(paths) =
                        self.rust_module(&file.path, module.scope, &module.name)
                    {
                        for path in paths {
                            if path == file.path {
                                continue;
                            }
                            if let Some(target) = self.files.get(&path) {
                                matches.extend(
                                    target
                                        .declarations
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, item)| {
                                            item.parent.is_none()
                                                && item.name == member
                                                && item.reliable
                                        })
                                        .map(|(index, _)| (path.clone(), index)),
                                );
                            }
                        }
                    }
                }
                collapse(matches)
            }
            BindingResolution::External { package_name, name } => BindingResolution::External {
                package_name,
                name: format!("{name}::{member}"),
            },
            BindingResolution::Candidates(keys) => {
                let mut members = Vec::new();
                for key in keys {
                    match self.resolve_member(BindingResolution::Local(key), member) {
                        BindingResolution::Local(key) => members.push(key),
                        BindingResolution::Candidates(keys) => members.extend(keys),
                        _ => {}
                    }
                }
                members.sort();
                members.dedup();
                if members.is_empty() {
                    unknown()
                } else {
                    BindingResolution::Candidates(members)
                }
            }
            unknown => unknown,
        }
    }

    /// 明确 this/实例/类型接收者的空间可消除 static 与 instance 同名成员歧义。
    pub(crate) fn resolve_receiver_member(
        &self,
        owner: BindingResolution,
        member: &str,
        static_receiver: bool,
    ) -> BindingResolution {
        if let BindingResolution::Local(key) = &owner {
            if self.this_member_reassigned(key, static_receiver, member) {
                return unknown();
            }
        }
        self.restrict_receiver(self.resolve_member(owner, member), static_receiver)
    }

    /// 已知成员写入只影响对应类与成员空间；计算属性写入保守覆盖同空间全部成员。
    fn this_member_reassigned(
        &self,
        owner: &DeclarationKey,
        static_receiver: bool,
        member: &str,
    ) -> bool {
        self.this_mutations
            .get(&(owner.clone(), static_receiver))
            .is_some_and(|members| {
                members.contains(&None) || members.contains(&Some(member.to_owned()))
            })
    }

    /// 只有拥有本地类/interface 定义时才检查 static，Rust关联函数语义不套用JS限制。
    fn restrict_receiver(
        &self,
        resolution: BindingResolution,
        static_receiver: bool,
    ) -> BindingResolution {
        let accepts = |key: &DeclarationKey| {
            let file = self.files[&key.0];
            let item = &file.declarations[key.1];
            if file.path.ends_with(".rs") {
                return true;
            }
            if item
                .parent
                .and_then(|parent| file.declarations.get(parent))
                .is_some_and(|parent| {
                    matches!(
                        parent.native_kind.category,
                        StructureCategory::Class | StructureCategory::Interface
                    )
                })
            {
                let owner = (key.0.clone(), item.parent.unwrap());
                let mutated = self.this_member_reassigned(&owner, static_receiver, &item.name);
                !mutated && is_static_signature(&item.signature) == static_receiver
            } else {
                true
            }
        };
        match resolution {
            BindingResolution::Local(key) => {
                if accepts(&key) {
                    BindingResolution::Local(key)
                } else {
                    unknown()
                }
            }
            BindingResolution::Candidates(keys) => {
                let rust = keys.iter().any(|key| key.0.ends_with(".rs"));
                let keys: Vec<_> = keys.into_iter().filter(accepts).collect();
                if rust && !keys.is_empty() {
                    BindingResolution::Candidates(keys)
                } else {
                    collapse(keys)
                }
            }
            other => other,
        }
    }

    /// import 依赖保留文件端点；导入成员本身由 resolve_type 单独解析。
    pub(crate) fn resolve_import_module(
        &self,
        path: &str,
        import: &ExtractedImport,
    ) -> ModuleResolution {
        if path.ends_with(".rs") {
            let module = if import.module.is_empty() {
                import.imported.as_str()
            } else {
                import.module.as_str()
            };
            self.rust_module(path, import.scope, module)
        } else if import.module.is_empty() {
            ModuleResolution::Files(vec![path.to_owned()])
        } else {
            self.ts_module(path, &import.module)
        }
    }

    /// 递归展开仅显式路径；循环 reexport 返回未知而不是无限递归。
    fn resolve(
        &self,
        path: &str,
        scope: usize,
        written: &str,
        offset: Option<usize>,
        visited: &mut HashSet<String>,
    ) -> BindingResolution {
        let Some(file) = self.files.get(path) else {
            return unknown();
        };
        let Some(name) = normalize_type(written) else {
            return unknown();
        };
        let visit = format!("{path}:{scope}:{name}");
        if !visited.insert(visit.clone()) {
            return unknown();
        }
        let result = self.resolve_inner(file, scope, &name, offset, visited);
        visited.remove(&visit);
        result
    }

    /// 先检查当前词法链；只有明确路径才进入模块索引。
    fn resolve_inner(
        &self,
        file: &ExtractedFile,
        scope: usize,
        name: &str,
        offset: Option<usize>,
        visited: &mut HashSet<String>,
    ) -> BindingResolution {
        let path = file.path.as_str();
        let (head, tail) = split_head(name);
        if let Some(bindings) = self.nearest_bindings(path, scope, head, offset) {
            let mut keys = Vec::new();
            let mut imports = Vec::new();
            for binding in bindings {
                if let Some(index) = binding.declaration {
                    if file
                        .declarations
                        .get(index)
                        .is_some_and(|item| item.reliable)
                    {
                        keys.push((file.path.clone(), index));
                    } else {
                        return BindingResolution::Unknown(
                            StructureDiagnosticReason::UnreliableScope,
                        );
                    }
                } else if binding.kind == ExtractedBindingKind::Import {
                    imports.extend(
                        file.imports
                            .iter()
                            .filter(|import| import.scope == binding.scope && import.local == head),
                    );
                } else {
                    return unknown();
                }
            }
            if !keys.is_empty() {
                let binding = collapse(keys);
                return tail.map_or(binding.clone(), |tail| self.resolve_members(binding, tail));
            }
            if !imports.is_empty() {
                return self.resolve_imports(file, imports, tail, visited);
            }
        }
        let mut current = Some(scope);
        while let Some(scope) = current {
            let Some(context) = file.scopes.get(scope) else {
                return BindingResolution::Unknown(StructureDiagnosticReason::UnreliableScope);
            };
            if !context.reliable {
                return BindingResolution::Unknown(StructureDiagnosticReason::UnreliableScope);
            }
            let imports: Vec<_> = file
                .imports
                .iter()
                .filter(|import| {
                    !import.reexport
                        && import.scope == scope
                        && import.local == head
                        && import.local != "*"
                })
                .collect();
            if !imports.is_empty() {
                return self.resolve_imports(file, imports, tail, visited);
            }
            current = context.parent;
        }
        if path.ends_with(".rs") && self.prelude_enabled(file, scope) {
            if let Some(target) = core_prelude(head) {
                let owner = BindingResolution::External {
                    package_name: "core".to_owned(),
                    name: target.to_owned(),
                };
                return tail.map_or(owner.clone(), |tail| self.resolve_members(owner, tail));
            }
        }
        if path.ends_with(".rs")
            && (name.contains("::") || matches!(name, "crate" | "self" | "super"))
        {
            return self.rust_symbol(path, scope, name, visited);
        }
        unknown()
    }

    /// 隐式 prelude 只作为最后兜底；glob 或任何已知禁用域都不提供唯一性证明。
    fn prelude_enabled(&self, file: &ExtractedFile, scope: usize) -> bool {
        let (root, _) = self.file_module(&file.path);
        if !file.implicit_prelude
            || !self.files.contains_key(&root)
            || self.rust_no_prelude.contains(&root)
        {
            return false;
        }
        let mut current = Some(scope);
        while let Some(scope) = current {
            if file
                .imports
                .iter()
                .any(|import| import.scope == scope && import.imported == "*")
            {
                return false;
            }
            current = file.scopes.get(scope).and_then(|scope| scope.parent);
        }
        true
    }

    /// scope 自身损坏时不越过它继续绑定；Rust 局部变量按源码顺序生效。
    fn nearest_bindings(
        &self,
        path: &str,
        scope: usize,
        name: &str,
        offset: Option<usize>,
    ) -> Option<Vec<&'a ExtractedBinding>> {
        let file = self.files.get(path)?;
        let mut current = Some(scope);
        while let Some(scope) = current {
            let context = file.scopes.get(scope)?;
            if !context.reliable {
                return None;
            }
            let mut bindings: Vec<_> = self
                .bindings
                .get(path)
                .and_then(|scopes| scopes.get(&scope))
                .and_then(|names| names.get(name))
                .into_iter()
                .flatten()
                .copied()
                .filter(|binding| {
                    !(path.ends_with(".rs")
                        && binding.kind == ExtractedBindingKind::Local
                        && offset.is_some_and(|offset| binding.range.bytes.start > offset))
                })
                .collect();
            if !bindings.is_empty() {
                if path.ends_with(".rs")
                    && bindings
                        .iter()
                        .any(|item| item.kind == ExtractedBindingKind::Local)
                {
                    let latest = bindings
                        .iter()
                        .filter(|item| item.kind == ExtractedBindingKind::Local)
                        .map(|item| item.range.bytes.start)
                        .max()
                        .unwrap();
                    bindings.retain(|item| {
                        item.kind == ExtractedBindingKind::Local && item.range.bytes.start == latest
                    });
                }
                return Some(bindings);
            }
            current = context.parent;
        }
        None
    }

    /// 名称空间尾部逐层处理；任一未知层都停止推断。
    fn resolve_members(&self, mut owner: BindingResolution, tail: &str) -> BindingResolution {
        for part in tail.replace("::", ".").split('.') {
            owner = self.resolve_member(owner, part);
        }
        owner
    }

    /// 同一词法名存在多个导入时保留候选，不依赖遍历顺序选目标。
    fn resolve_imports(
        &self,
        file: &ExtractedFile,
        imports: Vec<&ExtractedImport>,
        tail: Option<&str>,
        visited: &mut HashSet<String>,
    ) -> BindingResolution {
        let mut results = Vec::new();
        for import in imports {
            let result = if file.path.ends_with(".rs") {
                let full = if import.imported == "self" {
                    import.module.clone()
                } else if import.module.is_empty() {
                    import.imported.clone()
                } else {
                    format!("{}::{}", import.module, import.imported)
                };
                let qualified = tail.map_or(full.clone(), |tail| format!("{full}::{tail}"));
                if import.module.is_empty() && import.imported != import.local {
                    self.resolve(&file.path, import.scope, &qualified, None, visited)
                } else {
                    self.rust_symbol(&file.path, import.scope, &qualified, visited)
                }
            } else if import.module.is_empty() {
                self.resolve(&file.path, import.scope, &import.imported, None, visited)
            } else {
                match self.ts_module(&file.path, &import.module) {
                    ModuleResolution::Files(paths) => {
                        let target = if import.imported == "*" {
                            tail.unwrap_or("")
                        } else {
                            &import.imported
                        };
                        let found = combine(
                            paths
                                .iter()
                                .map(|path| self.exported(path, target, visited))
                                .collect(),
                        );
                        if import.imported == "*" {
                            found
                        } else {
                            tail.map_or(found.clone(), |tail| self.resolve_members(found, tail))
                        }
                    }
                    ModuleResolution::External(package_name) => {
                        let name = if import.imported == "*" || import.imported == "default" {
                            tail.map_or(import.imported.clone(), str::to_owned)
                        } else {
                            tail.map_or(import.imported.clone(), |tail| {
                                format!("{}.{tail}", import.imported)
                            })
                        };
                        let name = import
                            .module
                            .strip_prefix(&package_name)
                            .filter(|suffix| !suffix.is_empty())
                            .map_or(name.clone(), |suffix| {
                                format!("{}::{name}", suffix.trim_start_matches('/'))
                            });
                        BindingResolution::External { package_name, name }
                    }
                    ModuleResolution::Unknown(reason) => BindingResolution::Unknown(reason),
                }
            };
            results.push(result);
        }
        combine(results)
    }

    /// 只允许导出声明或显式 reexport；星号导出不制造唯一性证明。
    fn exported(&self, path: &str, name: &str, visited: &mut HashSet<String>) -> BindingResolution {
        let Some(file) = self.files.get(path) else {
            return unknown();
        };
        let visit = format!("export:{path}:{name}");
        if !visited.insert(visit.clone()) {
            return unknown();
        }
        let (head, tail) = split_head(name);
        let keys: Vec<_> = file
            .declarations
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.exported && item.parent.is_none() && item.name == head && item.reliable
            })
            .map(|(index, _)| (path.to_owned(), index))
            .collect();
        let result = if !keys.is_empty() {
            let found = collapse(keys);
            tail.map_or(found.clone(), |tail| self.resolve_members(found, tail))
        } else {
            let imports = file
                .imports
                .iter()
                .filter(|import| import.reexport && import.local == head)
                .collect();
            self.resolve_imports(file, imports, tail, visited)
        };
        visited.remove(&visit);
        result
    }

    /// 相对路径和当前配置中的 aliases 才能命中仓库文件，裸包名保留外部身份。
    fn ts_module(&self, path: &str, module: &str) -> ModuleResolution {
        if module.starts_with('.') {
            return self.ts_files(&join(dirname(path), module));
        }
        let config = self
            .configs
            .iter()
            .find(|config| within(path, &config.directory));
        if let Some(config) = config {
            if !config.valid {
                return ModuleResolution::Unknown(
                    StructureDiagnosticReason::ConfigurationUnavailable,
                );
            }
            for (pattern, targets) in &config.paths {
                if let Some(middle) = wildcard_match(pattern, module) {
                    let mut paths = Vec::new();
                    for target in targets {
                        if let ModuleResolution::Files(found) =
                            self.ts_files(&join(&config.base, &target.replace('*', &middle)))
                        {
                            paths.extend(found);
                        }
                    }
                    paths.sort();
                    paths.dedup();
                    return if paths.is_empty() {
                        ModuleResolution::Unknown(
                            StructureDiagnosticReason::ConfigurationUnavailable,
                        )
                    } else {
                        ModuleResolution::Files(paths)
                    };
                }
            }
            if let ModuleResolution::Files(paths) = self.ts_files(&join(&config.base, module)) {
                return ModuleResolution::Files(paths);
            }
        }
        if module.starts_with("@/")
            || module.starts_with("~/")
            || module.starts_with('#')
            || module.starts_with('/')
        {
            return ModuleResolution::Unknown(StructureDiagnosticReason::ConfigurationUnavailable);
        }
        ModuleResolution::External(package_name(module))
    }

    /// 扩展名替换覆盖 ESM 写 .js 导入 .ts，多个可行文件保留歧义。
    fn ts_files(&self, base: &str) -> ModuleResolution {
        if self.files.contains_key(base) {
            return ModuleResolution::Files(vec![base.to_owned()]);
        }
        let stem = base
            .strip_suffix(".js")
            .or_else(|| base.strip_suffix(".jsx"))
            .unwrap_or(base);
        let mut paths = Vec::new();
        for extension in [
            ".ts",
            ".tsx",
            ".js",
            ".jsx",
            ".mts",
            ".cts",
            ".mjs",
            ".cjs",
            ".d.ts",
            "/index.ts",
            "/index.tsx",
            "/index.js",
            "/index.jsx",
        ] {
            let path = format!("{stem}{extension}");
            if self.files.contains_key(&path) {
                paths.push(path);
            }
        }
        if paths.is_empty() {
            ModuleResolution::Unknown(StructureDiagnosticReason::ConfigurationUnavailable)
        } else {
            ModuleResolution::Files(paths)
        }
    }

    /// 模块身份来自真实 mod 图；同文件被多个 crate/include 复用时不猜执行上下文。
    fn file_module(&self, path: &str) -> (String, String) {
        match self.rust_locations.get(path).map(Vec::as_slice) {
            Some([location]) => location.clone(),
            _ => (format!("unlinked:{path}"), String::new()),
        }
    }

    /// 以 Cargo 根及声明的 mod 边遍历，物理 tests 目录不会凭文件名伪装逻辑模块。
    fn build_rust_modules(&mut self, config: &HashMap<String, String>) {
        let mut roots = rust_roots(&self.files, config);
        roots.sort();
        roots.dedup();
        let mut queue: Vec<_> = roots
            .into_iter()
            .map(|path| (path.clone(), path, String::new()))
            .collect();
        let mut visited = HashSet::new();
        let mut expanded = HashSet::new();
        while let Some((path, root, module)) = queue.pop() {
            if !visited.insert((path.clone(), root.clone(), module.clone())) {
                continue;
            }
            let Some(file) = self.files.get(&path).copied() else {
                continue;
            };
            if !file.implicit_prelude {
                self.rust_no_prelude.insert(root.clone());
            }
            self.rust_modules
                .entry((root.clone(), module.clone()))
                .or_default()
                .push(path.clone());
            self.rust_locations
                .entry(path.clone())
                .or_default()
                .push((root.clone(), module.clone()));
            if !expanded.insert((path.clone(), root.clone())) {
                continue;
            }
            for declaration in file.declarations.iter().filter(|item| {
                item.native_kind.category == StructureCategory::Module
                    && item.reliable
                    && item.signature.trim_end().ends_with(';')
            }) {
                let mut parents = Vec::new();
                let mut current = declaration.parent;
                let mut reliable = true;
                while let Some(parent) = current {
                    let parent = &file.declarations[parent];
                    if parent.native_kind.category != StructureCategory::Module
                        || matches!(parent.module_path, Some(ExtractedModulePath::Unknown))
                    {
                        reliable = false;
                        break;
                    }
                    // 内联模块自身的 path 属性需要额外目录语义；不猜其后代物理路径。
                    if matches!(parent.module_path, Some(ExtractedModulePath::Explicit(_))) {
                        reliable = false;
                        break;
                    }
                    parents.push(parent.name.clone());
                    current = parent.parent;
                }
                if !reliable {
                    continue;
                }
                parents.reverse();
                let mut logical = if module.is_empty() {
                    Vec::new()
                } else {
                    module.split("::").map(str::to_owned).collect()
                };
                logical.extend(parents.iter().cloned());
                logical.push(declaration.name.clone());
                let default_directory = if path == root {
                    dirname(&path).to_owned()
                } else {
                    rust_module_directory(&path)
                };
                let targets = match declaration.module_path.as_ref() {
                    Some(ExtractedModulePath::Default) => {
                        let directory = join(&default_directory, &parents.join("/"));
                        vec![
                            join(&directory, &format!("{}.rs", declaration.name)),
                            join(&directory, &format!("{}/mod.rs", declaration.name)),
                        ]
                    }
                    Some(ExtractedModulePath::Explicit(target)) => {
                        let directory = if parents.is_empty() {
                            dirname(&path).to_owned()
                        } else {
                            join(&default_directory, &parents.join("/"))
                        };
                        vec![join(&directory, target)]
                    }
                    _ => Vec::new(),
                };
                for target in targets
                    .into_iter()
                    .filter(|target| self.files.contains_key(target))
                {
                    queue.push((target, root.clone(), logical.join("::")));
                }
            }
        }
        for paths in self.rust_modules.values_mut() {
            paths.sort();
            paths.dedup();
        }
        for locations in self.rust_locations.values_mut() {
            locations.sort();
            locations.dedup();
        }
    }

    /// Rust 模块坐标由同侧文件及 inline mod 祖先组成，crate/self/super 不跨 crate。
    fn rust_context(&self, path: &str, scope: usize) -> (String, Vec<String>) {
        let (root, module) = self.file_module(path);
        let mut segments: Vec<String> = module
            .split("::")
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect();
        if let Some(file) = self.files.get(path) {
            let mut current = Some(scope);
            let mut inline = Vec::new();
            let mut seen = HashSet::new();
            while let Some(scope) = current {
                let Some(context) = file.scopes.get(scope) else {
                    break;
                };
                if let Some(declaration) = context
                    .declaration
                    .and_then(|index| file.declarations.get(index))
                {
                    if declaration.native_kind.category == StructureCategory::Module {
                        if seen.insert(context.declaration.unwrap()) {
                            inline.push(declaration.name.clone());
                        }
                    }
                }
                current = context.parent;
            }
            inline.reverse();
            segments.extend(inline);
        }
        (root, segments)
    }

    /// 解析 Rust 的锚定模块；裸首段仅在本 crate 明确存在时作为本地模块。
    fn rust_module(&self, path: &str, scope: usize, written: &str) -> ModuleResolution {
        let (root, mut context) = self.rust_context(path, scope);
        let absolute = written.starts_with("::");
        let mut parts = written
            .trim_start_matches("::")
            .split("::")
            .filter(|part| !part.is_empty())
            .peekable();
        match parts.peek().copied() {
            Some("crate") => {
                context.clear();
                parts.next();
            }
            Some("self") => {
                parts.next();
            }
            Some("super") => {
                while parts.peek() == Some(&"super") {
                    context.pop();
                    parts.next();
                }
            }
            Some(_) if absolute => {
                let package = written
                    .trim_start_matches("::")
                    .split("::")
                    .next()
                    .unwrap_or("");
                return if self.known_rust_package(path, package) {
                    ModuleResolution::External(package.to_owned())
                } else {
                    ModuleResolution::Unknown(StructureDiagnosticReason::ConfigurationUnavailable)
                };
            }
            _ => {}
        }
        context.extend(parts.map(str::to_owned));
        let module = context.join("::");
        if let Some(paths) = self.rust_modules.get(&(root.clone(), module.clone())) {
            return ModuleResolution::Files(paths.clone());
        }
        // inline mod 没有独立文件；从最深的物理父模块回到声明所在文件。
        for count in (0..context.len()).rev() {
            if let Some(paths) = self
                .rust_modules
                .get(&(root.clone(), context[..count].join("::")))
            {
                let suffix = &context[count..];
                let paths: Vec<_> = paths
                    .iter()
                    .filter(|path| self.find_inline_module(path, suffix).is_some())
                    .cloned()
                    .collect();
                if !paths.is_empty() {
                    return ModuleResolution::Files(paths);
                }
            }
        }
        if written.starts_with("crate::")
            || written.starts_with("self::")
            || written.starts_with("super::")
            || matches!(written, "crate" | "self" | "super")
        {
            ModuleResolution::Unknown(StructureDiagnosticReason::ConfigurationUnavailable)
        } else {
            let package = written.split("::").next().unwrap_or(written);
            if self.known_rust_package(path, package) {
                ModuleResolution::External(package.to_owned())
            } else {
                ModuleResolution::Unknown(StructureDiagnosticReason::ConfigurationUnavailable)
            }
        }
    }

    /// 只有当前 Cargo 配置或语言自带 crate 才提供仓库外包证据。
    fn known_rust_package(&self, path: &str, package: &str) -> bool {
        matches!(package, "std" | "core" | "alloc")
            || self
                .rust_dependencies
                .iter()
                .filter(|(directory, _)| within(path, directory))
                .max_by_key(|(directory, _)| directory.len())
                .is_some_and(|(_, dependencies)| dependencies.contains(package))
    }

    /// 内联模块必须沿真实 parent 链逐层匹配，不能借用其他模块里的同名声明。
    fn find_inline_module(&self, path: &str, parts: &[String]) -> Option<usize> {
        let file = self.files.get(path)?;
        let mut parent = None;
        for part in parts {
            parent = Some(file.declarations.iter().position(|item| {
                item.name == *part
                    && item.parent == parent
                    && item.native_kind.category == StructureCategory::Module
                    && !matches!(item.module_path, Some(ExtractedModulePath::Unknown))
                    && !item.signature.trim_end().ends_with(';')
                    && item.reliable
            })?);
        }
        parent
    }

    /// Rust 重导出循环和类型别名循环只产生未知，不递归耗尽线程栈。
    fn rust_symbol(
        &self,
        path: &str,
        scope: usize,
        written: &str,
        visited: &mut HashSet<String>,
    ) -> BindingResolution {
        let visit = format!("rust:{path}:{scope}:{written}");
        if !visited.insert(visit.clone()) {
            return unknown();
        }
        let result = self.rust_symbol_inner(path, scope, written, visited);
        visited.remove(&visit);
        result
    }

    /// 限定符先定位模块，再在该模块中按显式声明/重导出逐层查找。
    fn rust_symbol_inner(
        &self,
        path: &str,
        scope: usize,
        written: &str,
        visited: &mut HashSet<String>,
    ) -> BindingResolution {
        let Some((module, symbol)) = written.rsplit_once("::") else {
            return match self.rust_module(path, scope, written) {
                ModuleResolution::External(package_name) => BindingResolution::External {
                    package_name,
                    name: written.to_owned(),
                },
                _ => unknown(),
            };
        };
        match self.rust_module(path, scope, module) {
            ModuleResolution::External(package_name) => BindingResolution::External {
                package_name,
                name: written.trim_start_matches("::").to_owned(),
            },
            ModuleResolution::Unknown(reason) => {
                // crate::types::Service::run 的尾部可能是类型成员而非模块。
                if let Some((owner, member)) = written.rsplit_once("::") {
                    let resolved = self.resolve(path, scope, owner, None, visited);
                    if !matches!(resolved, BindingResolution::Unknown(_)) {
                        return self.resolve_member(resolved, member);
                    }
                }
                BindingResolution::Unknown(reason)
            }
            ModuleResolution::Files(paths) => {
                let (root, context) = self.rust_context(path, scope);
                let _ = root;
                let mut parts: Vec<String> = module.split("::").map(str::to_owned).collect();
                if parts.first().is_some_and(|part| part == "crate") {
                    parts.remove(0);
                } else if parts.first().is_some_and(|part| part == "self") {
                    parts.remove(0);
                    parts.splice(0..0, context);
                } else if parts.first().is_some_and(|part| part == "super") {
                    let mut context = context;
                    while parts.first().is_some_and(|part| part == "super") {
                        parts.remove(0);
                        context.pop();
                    }
                    parts.splice(0..0, context);
                } else {
                    parts.splice(0..0, context);
                }
                let mut results = Vec::new();
                for target_path in paths {
                    let file = self.files[&target_path];
                    let (_, physical) = self.file_module(&target_path);
                    let physical_count =
                        physical.split("::").filter(|part| !part.is_empty()).count();
                    let suffix = parts.get(physical_count..).unwrap_or(&[]);
                    let parent = if suffix.is_empty() {
                        None
                    } else {
                        self.find_inline_module(&target_path, suffix)
                    };
                    if !suffix.is_empty() && parent.is_none() {
                        continue;
                    }
                    let keys: Vec<_> = file
                        .declarations
                        .iter()
                        .enumerate()
                        .filter(|(_, item)| {
                            item.parent == parent
                                && item.name == symbol
                                && item.reliable
                                && item.impl_group.is_none()
                        })
                        .map(|(index, _)| (target_path.clone(), index))
                        .collect();
                    if !keys.is_empty() {
                        results.push(collapse(keys));
                    } else {
                        let imports = file
                            .imports
                            .iter()
                            .filter(|import| {
                                import.reexport
                                    && import.local == symbol
                                    && parent.map_or(import.scope == 0, |parent| {
                                        file.scopes
                                            .get(import.scope)
                                            .is_some_and(|scope| scope.declaration == Some(parent))
                                    })
                            })
                            .collect();
                        results.push(self.resolve_imports(file, imports, None, visited));
                    }
                }
                combine(results)
            }
        }
    }
}

/// 从真实作用域找词法this所属类与static空间，缺少成员上下文时保守覆盖两种空间。
fn class_this_owner(file: &ExtractedFile, scope: usize) -> Option<(DeclarationKey, Option<bool>)> {
    let mut current = Some(scope);
    let mut namespace = None;
    let mut visited = HashSet::new();
    while let Some(scope) = current {
        if !visited.insert(scope) {
            return None;
        }
        let context = file.scopes.get(scope)?;
        if !context.reliable {
            return None;
        }
        if let Some(index) = context.declaration {
            let declaration = file.declarations.get(index)?;
            if namespace.is_none()
                && matches!(
                    declaration.native_kind.category,
                    StructureCategory::Method | StructureCategory::Field
                )
            {
                namespace = Some(is_static_signature(&declaration.signature));
            }
            if declaration.native_kind.category == StructureCategory::Class {
                return Some(((file.path.clone(), index), namespace));
            }
        }
        current = context.parent;
    }
    None
}

/// static 修饰只检查成员头部，参数或正文里的同名字符串不会提供静态成员证据。
pub(super) fn is_static_signature(signature: &str) -> bool {
    signature
        .split(['(', '=', ':'])
        .next()
        .unwrap_or("")
        .split_whitespace()
        .any(|part| part == "static")
}

/// 类型参数不参与名义类型身份，但复杂联合、dyn 与计算表达式不猜测目标。
fn normalize_type(written: &str) -> Option<String> {
    let mut text = written.trim();
    if text.starts_with('<') {
        return None;
    }
    while text.starts_with('&') {
        text = text[1..].trim_start();
        if text.starts_with('\'') {
            text = text.split_once(char::is_whitespace)?.1.trim_start();
        }
    }
    text = text.strip_prefix("mut ").unwrap_or(text).trim();
    text = text.strip_prefix("type ").unwrap_or(text).trim();
    if text.starts_with("dyn ") || text.starts_with("impl ") {
        return None;
    }
    let mut result = String::new();
    let mut depth = 0usize;
    for ch in text.chars() {
        match ch {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            ch if ch.is_whitespace() => {}
            ch if ch.is_alphanumeric() || matches!(ch, '_' | '$' | ':' | '.') => result.push(ch),
            _ => return None,
        }
    }
    if depth != 0 || result.is_empty() {
        None
    } else {
        Some(result.trim_end_matches("::").to_owned())
    }
}

/// 首段与成员后缀使用原字符串借用，兼容 TS 点路径及 Rust ::。
fn split_head(name: &str) -> (&str, Option<&str>) {
    if let Some(index) = name.find(['.', ':']) {
        (
            &name[..index],
            Some(name[index..].trim_start_matches(['.', ':'])),
        )
    } else {
        (name, None)
    }
}

/// 接收者调用仅拆最末端，复杂接收者由规范化规则返回未知。
fn split_member(name: &str) -> Option<(&str, &str)> {
    name.rsplit_once('.').or_else(|| name.rsplit_once("::"))
}

/// 合并同名声明保留所有候选，空集合表示证据不足。
fn collapse(mut keys: Vec<DeclarationKey>) -> BindingResolution {
    keys.sort();
    keys.dedup();
    match keys.len() {
        0 => unknown(),
        1 => BindingResolution::Local(keys.remove(0)),
        _ => BindingResolution::Candidates(keys),
    }
}

/// 任何未覆盖分支都会阻止唯一性结论，已知候选仍可供用户检查。
fn combine(results: Vec<BindingResolution>) -> BindingResolution {
    if results.len() == 1 {
        return results.into_iter().next().unwrap();
    }
    let mut keys = Vec::new();
    let mut external = Vec::new();
    let mut incomplete = false;
    for result in results {
        match result {
            BindingResolution::Local(key) => keys.push(key),
            BindingResolution::Candidates(found) => {
                keys.extend(found);
                incomplete = true;
            }
            BindingResolution::External { package_name, name } => {
                external.push((package_name, name))
            }
            BindingResolution::Unknown(_) => incomplete = true,
        }
    }
    keys.sort();
    keys.dedup();
    external.sort();
    external.dedup();
    if !keys.is_empty() {
        if incomplete || !external.is_empty() {
            BindingResolution::Candidates(keys)
        } else {
            collapse(keys)
        }
    } else if external.len() == 1 && !incomplete {
        let (package_name, name) = external.remove(0);
        BindingResolution::External { package_name, name }
    } else {
        unknown()
    }
}

/// 无明确静态证据时使用动态目标原因，而不是造出本地节点。
fn unknown() -> BindingResolution {
    BindingResolution::Unknown(StructureDiagnosticReason::DynamicTarget)
}

/// 仓库路径规范化不接触文件系统，越过根目录的路径不会命中文件。
fn join(base: &str, tail: &str) -> String {
    let combined = if base.is_empty() {
        tail.to_owned()
    } else {
        format!("{base}/{tail}")
    };
    let mut parts = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|part| *part != "..") => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

/// 获取仓库相对父目录。
fn dirname(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// 获取配置文件基本名称。
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// 配置作用域必须跨越完整目录分隔符。
fn within(path: &str, directory: &str) -> bool {
    directory.is_empty()
        || path
            .strip_prefix(directory)
            .is_some_and(|tail| tail.starts_with('/'))
}

/// 路径别名支持 TypeScript 的单个星号，不依赖散列遍历顺序。
fn wildcard_match(pattern: &str, module: &str) -> Option<String> {
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        module
            .strip_prefix(prefix)?
            .strip_suffix(suffix)
            .map(str::to_owned)
    } else {
        (pattern == module).then(String::new)
    }
}

/// npm scoped 包身份保留 scope，包内子路径仍归于同一外部包。
fn package_name(module: &str) -> String {
    let mut parts = module.split('/');
    let first = parts.next().unwrap_or(module);
    if first.starts_with('@') {
        parts
            .next()
            .map_or(first.to_owned(), |name| format!("{first}/{name}"))
    } else {
        first.to_owned()
    }
}

/// 常规外部模块的子模块目录来自源码文件位置，crate 根由调用者单独处理。
fn rust_module_directory(path: &str) -> String {
    if basename(path) == "mod.rs" {
        dirname(path).to_owned()
    } else {
        path.strip_suffix(".rs").unwrap_or(path).to_owned()
    }
}

/// Cargo 显式 source target 只接受当前配置中的字面量路径；不可解释的自定义域不回退猜默认根。
fn rust_roots(
    files: &HashMap<String, &ExtractedFile>,
    configs: &HashMap<String, String>,
) -> Vec<String> {
    let mut roots = Vec::new();
    let mut customized = HashSet::new();
    for (config_path, text) in configs
        .iter()
        .filter(|(path, _)| basename(path) == "Cargo.toml")
    {
        let directory = dirname(config_path);
        let mut target_section = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                let section = line.trim_start_matches('[').trim_end_matches(']');
                target_section = matches!(section, "lib" | "bin" | "test" | "example" | "bench");
            } else if target_section {
                if let Some((name, value)) = line.split_once('=') {
                    if name.trim() == "path" {
                        customized.insert(directory.to_owned());
                        if let Some(path) = toml_literal(value.trim()) {
                            let path = join(directory, &path);
                            if files.contains_key(&path) {
                                roots.push(path);
                            }
                        }
                    }
                }
            }
        }
    }
    for path in files.keys().filter(|path| path.ends_with(".rs")) {
        let directory = dirname(path);
        let default_root = matches!(basename(path), "lib.rs" | "main.rs")
            && (directory.is_empty() || basename(directory) == "src");
        let cargo_directory = configs
            .keys()
            .filter(|config| basename(config) == "Cargo.toml" && within(path, dirname(config)))
            .map(|config| dirname(config))
            .max_by_key(|directory| directory.len());
        if default_root && !cargo_directory.is_some_and(|directory| customized.contains(directory))
        {
            roots.push(path.clone());
        }
    }
    roots
}

/// Cargo 普通/字面量字符串保留转义；路径后的注释不进入文件名。
fn toml_literal(value: &str) -> Option<String> {
    if let Some(value) = value.strip_prefix('\'') {
        return value
            .split_once('\'')
            .map(|(literal, _)| literal.to_owned());
    }
    if !value.starts_with('"') {
        return None;
    }
    let mut escaped = false;
    for (index, ch) in value.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return serde_json::from_str(&value[..=index]).ok();
        }
    }
    None
}

/// 读取 JSONC 配置与相对 extends；循环或不可读继承使别名覆盖未知。
fn load_tsconfig(
    path: &str,
    configs: &HashMap<String, String>,
    visiting: &mut HashSet<String>,
) -> TsConfig {
    let directory = dirname(path).to_owned();
    let mut config = TsConfig {
        directory: directory.clone(),
        base: directory,
        ..TsConfig::default()
    };
    if !visiting.insert(path.to_owned()) {
        return config;
    }
    if let Some(value) = configs
        .get(path)
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&jsonc(text)).ok())
    {
        config.valid = true;
        if let Some(extends) = value.get("extends").and_then(|value| value.as_str()) {
            if extends.starts_with('.') {
                let mut parent = join(&config.directory, extends);
                if !parent.ends_with(".json") {
                    parent.push_str(".json");
                }
                let inherited = load_tsconfig(&parent, configs, visiting);
                config.valid = inherited.valid;
                config.base = inherited.base;
                config.paths = inherited.paths;
            } else {
                config.valid = false;
            }
        }
        if let Some(options) = value.get("compilerOptions") {
            if let Some(base) = options.get("baseUrl").and_then(|value| value.as_str()) {
                config.base = join(&config.directory, base);
            }
            if let Some(paths) = options.get("paths").and_then(|value| value.as_object()) {
                config.paths = paths
                    .iter()
                    .map(|(pattern, targets)| {
                        (
                            pattern.clone(),
                            targets.as_array().map_or_else(Vec::new, |targets| {
                                targets
                                    .iter()
                                    .filter_map(|value| value.as_str().map(str::to_owned))
                                    .collect()
                            }),
                        )
                    })
                    .collect();
                config.paths.sort_by_key(|(pattern, _)| {
                    (
                        pattern.contains('*'),
                        std::cmp::Reverse(pattern.find('*').unwrap_or(pattern.len())),
                    )
                });
            }
        }
    }
    visiting.remove(path);
    config
}

/// 在字符串之外移除 JSONC 注释和尾逗号，字符串内的 URL 和转义保持原样。
fn jsonc(text: &str) -> String {
    let chars: Vec<_> = text.chars().collect();
    let mut result = String::new();
    let (mut index, mut quoted, mut escaped) = (0, false, false);
    while index < chars.len() {
        let ch = chars[index];
        if quoted {
            result.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            index += 1;
            continue;
        }
        if ch == '"' {
            quoted = true;
            result.push(ch);
            index += 1;
            continue;
        }
        if ch == '/' && chars.get(index + 1) == Some(&'/') {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if ch == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                index += 1;
            }
            index = (index + 2).min(chars.len());
            continue;
        }
        result.push(ch);
        index += 1;
    }
    let chars: Vec<_> = result.chars().collect();
    let mut output = String::new();
    quoted = false;
    escaped = false;
    for (index, ch) in chars.iter().copied().enumerate() {
        if quoted {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        if ch == '"' {
            quoted = true;
        }
        if ch == ','
            && chars[index + 1..]
                .iter()
                .find(|ch| !ch.is_whitespace())
                .is_some_and(|ch| matches!(ch, '}' | ']'))
        {
            continue;
        }
        output.push(ch);
    }
    output
}

/// 仅列出各版 core prelude 共有的明确符号；derive 宏不是同名 trait 的导入证据。
fn core_prelude(name: &str) -> Option<&'static str> {
    Some(match name {
        "Default" => "core::default::Default",
        "Clone" => "core::clone::Clone",
        "Copy" => "core::marker::Copy",
        "Send" => "core::marker::Send",
        "Sized" => "core::marker::Sized",
        "Sync" => "core::marker::Sync",
        "Unpin" => "core::marker::Unpin",
        "Drop" => "core::ops::Drop",
        "Fn" => "core::ops::Fn",
        "FnMut" => "core::ops::FnMut",
        "FnOnce" => "core::ops::FnOnce",
        "Eq" => "core::cmp::Eq",
        "Ord" => "core::cmp::Ord",
        "PartialEq" => "core::cmp::PartialEq",
        "PartialOrd" => "core::cmp::PartialOrd",
        "AsMut" => "core::convert::AsMut",
        "AsRef" => "core::convert::AsRef",
        "From" => "core::convert::From",
        "Into" => "core::convert::Into",
        "DoubleEndedIterator" => "core::iter::DoubleEndedIterator",
        "ExactSizeIterator" => "core::iter::ExactSizeIterator",
        "Extend" => "core::iter::Extend",
        "IntoIterator" => "core::iter::IntoIterator",
        "Iterator" => "core::iter::Iterator",
        "Option" => "core::option::Option",
        "None" => "core::option::Option::None",
        "Some" => "core::option::Option::Some",
        "Result" => "core::result::Result",
        "Ok" => "core::result::Result::Ok",
        "Err" => "core::result::Result::Err",
        "drop" => "core::mem::drop",
        _ => return None,
    })
}

#[cfg(test)]
#[path = "tests/repository_index.rs"]
mod tests;

/// Cargo 配置只读取依赖表中的本地 crate 名称，不扫描 registry 或解析版本内容。
fn cargo_dependencies(configs: &HashMap<String, String>) -> HashMap<String, HashSet<String>> {
    let mut result = HashMap::new();
    for (path, text) in configs
        .iter()
        .filter(|(path, _)| basename(path) == "Cargo.toml")
    {
        let mut dependencies = HashSet::new();
        let mut in_dependencies = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                let section = line.trim_start_matches('[').trim_end_matches(']');
                let parts: Vec<_> = section.split('.').collect();
                in_dependencies = parts.last().is_some_and(|part| {
                    matches!(
                        *part,
                        "dependencies" | "dev-dependencies" | "build-dependencies"
                    )
                });
                for (index, part) in parts.iter().enumerate() {
                    if matches!(
                        *part,
                        "dependencies" | "dev-dependencies" | "build-dependencies"
                    ) {
                        if let Some(name) = parts.get(index + 1) {
                            dependencies.insert(name.trim_matches(['"', '\'']).replace('-', "_"));
                        }
                    }
                }
            } else if in_dependencies {
                if let Some((name, _)) = line.split_once('=') {
                    let name = name.trim().trim_matches(['"', '\'']);
                    if !name.is_empty()
                        && name
                            .chars()
                            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-'))
                    {
                        dependencies.insert(name.replace('-', "_"));
                    }
                }
            }
        }
        result.insert(dirname(path).to_owned(), dependencies);
    }
    result
}
