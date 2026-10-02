using System.Security.Cryptography;
using System.Text;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;

namespace Narrant.CSharp.Helper;

/// <summary>分析结果同时包含完整图与输入覆盖证据。</summary>
internal sealed record AnalysisOutput(object Graph, Evidence Evidence);

/// <summary>以 Roslyn 编译器 API 分别分析静态配置并合并已证实的事实。</summary>
internal static class Analyzer
{
    /// <summary>验证输入、编译每个配置并完成单图并集。</summary>
    public static AnalysisOutput Analyze(AnalyzeRequest request, CancellationToken cancellation)
    {
        var input = request.Input;
        if (input.Profile != "csharp-static-v1" || !Path.IsPathFullyQualified(input.RepositoryRoot) ||
            !Directory.Exists(input.RepositoryRoot) || !Path.IsPathFullyQualified(request.Environment.ScratchRoot))
            throw new HelperException("invalid_input", "Invalid C# profile, repository, or scratch root");
        var root = Path.GetFullPath(input.RepositoryRoot).TrimEnd(Path.DirectorySeparatorChar);
        var selected = input.Files.ToHashSet(StringComparer.Ordinal);
        foreach (var file in selected) Configuration.CheckFile(root, file);
        var configurations = Configuration.Discover(root, cancellation);
        var platformIds = request.Environment.Platforms.Select(p => p.Id).ToArray();
        if (platformIds.Distinct(StringComparer.Ordinal).Count() != platformIds.Length)
            throw new HelperException("invalid_input", "Duplicate platform ids");
        var graph = new GraphAccumulator();
        var coverage = selected.ToDictionary(path => path, _ => new List<string>(), StringComparer.Ordinal);
        var excludedTests = selected.ToDictionary(path => path, _ => new List<string>(), StringComparer.Ordinal);
        var inactive = selected.ToDictionary(path => path, _ => new List<string>(), StringComparer.Ordinal);
        var configurationEvidence = new List<ConfigurationEvidence>();
        var compiled = new Dictionary<string, (CSharpCompilation Compilation, Dictionary<string, SyntaxTree> Trees, Platform Platform)>(StringComparer.Ordinal);
        var compiling = new HashSet<string>(StringComparer.Ordinal);
        foreach (var config in configurations.Where(c => input.IncludeTests || !c.TestOnly)) Compile(config);
        var projection = new ProjectionIndex(compiled.Values.SelectMany(value => value.Trees.Values));
        foreach (var config in configurations)
        {
            cancellation.ThrowIfCancellationRequested();
            if (config.TestOnly && !input.IncludeTests)
            {
                configurationEvidence.Add(new ConfigurationEvidence(config.Id, config.SourcePaths, []));
                foreach (var file in config.Files)
                    if (selected.Contains(file)) excludedTests[file].Add(config.Id);
                continue;
            }
            var (compilation, trees, platform) = Compile(config);
            configurationEvidence.Add(new ConfigurationEvidence(config.Id, config.SourcePaths, [platform.Id]));
            foreach (var file in config.Files)
                if (selected.Contains(file)) coverage[file].Add(config.Id);
            foreach (var file in config.InactiveFiles)
                if (selected.Contains(file)) inactive[file].Add(config.Id);
            var requiredPartialFailure = compilation.GetDiagnostics(cancellation).FirstOrDefault(d =>
                d.Id is "CS8795" or "CS8796" or "CS8797" or "CS0759" or "CS0766" or "CS8805");
            if (requiredPartialFailure is not null)
                throw new HelperException("object_extraction_failed", $"Incomplete partial declaration: {requiredPartialFailure}");
            var extracted = new List<ExtractedNode>();
            foreach (var (path, tree) in trees)
            {
                cancellation.ThrowIfCancellationRequested();
                var model = compilation.GetSemanticModel(tree);
                Extractor.Extract(path, tree, model, projection, selected.Contains(path), extracted, cancellation);
            }
            var duplicate = extracted.Where(n => n.Selected).GroupBy(n => n.Node.Id, StringComparer.Ordinal)
                .FirstOrDefault(group => group.Count() > 1);
            if (duplicate is not null)
                throw new HelperException("object_extraction_failed", $"Duplicate C# source identity {duplicate.Key}",
                    duplicate.First().Path);
            graph.Merge(extracted, cancellation);
        }
        foreach (var (path, ids) in coverage)
            if (ids.Count == 0 && excludedTests[path].Count == 0 && inactive[path].Count == 0)
                throw new HelperException("unsupported_configuration", "Selected C# file is outside all supported configurations", path);
        var files = input.Files.Select(path => coverage[path].Count > 0
            ? new FileEvidence(path, "analyzed", coverage[path].Order(StringComparer.Ordinal).ToArray())
            : excludedTests[path].Count > 0
                ? new FileEvidence(path, "excluded_test", excludedTests[path].Order(StringComparer.Ordinal).ToArray())
                : new FileEvidence(path, "inactive", inactive[path].Order(StringComparer.Ordinal).ToArray())).ToList();
        return graph.Finish(new Evidence(configurationEvidence, files, []));

        // 递归建立项目引用，只把仓库内项目 Compilation 作为编译时 metadata，不执行或 emit 它们。
        (CSharpCompilation Compilation, Dictionary<string, SyntaxTree> Trees, Platform Platform) Compile(StaticConfiguration config)
        {
            if (compiled.TryGetValue(config.Id, out var ready)) return ready;
            if (!compiling.Add(config.Id))
                throw new HelperException("unsupported_configuration", "ProjectReference cycle", config.SourcePaths);
            cancellation.ThrowIfCancellationRequested();
            var matches = request.Environment.Platforms.Where(p => p.Kind == "dotnet-reference-assemblies" &&
                p.Metadata.Tfm == config.Tfm && p.Metadata.ReferencePack == "Microsoft.NETCore.App.Ref" &&
                p.Metadata.ReferencePackVersion == "10.0.0").ToArray();
            if (matches.Length != 1)
                throw new HelperException("target_unavailable", $"Exactly one approved reference assembly profile is required for {config.Tfm}", config.SourcePaths);
            var platform = matches[0];
            var references = References(platform).ToList();
            foreach (var dependency in config.ProjectReferences)
            {
                var target = configurations.SingleOrDefault(c => c.SourcePaths[0] == dependency && c.Tfm == config.Tfm);
                if (target is null)
                    throw new HelperException("unsupported_configuration", $"ProjectReference lacks matching TFM: {dependency}", config.SourcePaths);
                if (target.TestOnly && !input.IncludeTests)
                    throw new HelperException("unsupported_configuration", "Production project references excluded test project", config.SourcePaths);
                references.Add(Compile(target).Compilation.ToMetadataReference());
            }
            var trees = new Dictionary<string, SyntaxTree>(StringComparer.Ordinal);
            foreach (var file in config.Files)
            {
                cancellation.ThrowIfCancellationRequested();
                string text;
                try { text = File.ReadAllText(Configuration.CheckFile(root, file), new UTF8Encoding(false, true)); }
                catch (Exception error) when (error is IOException or UnauthorizedAccessException or DecoderFallbackException)
                { throw new HelperException("object_extraction_failed", $"Cannot read UTF-8 source: {error.Message}", file); }
                var options = new CSharpParseOptions(config.LanguageVersion, preprocessorSymbols: config.Defines);
                var tree = CSharpSyntaxTree.ParseText(text, options, path: file, cancellationToken: cancellation);
                var syntaxError = tree.GetDiagnostics(cancellation).FirstOrDefault(d => d.Severity == DiagnosticSeverity.Error);
                if (syntaxError is not null)
                    throw new HelperException("object_extraction_failed", $"Invalid C# syntax: {syntaxError}", file);
                trees[file] = tree;
            }
            var allTrees = trees.Values.ToList();
            if (config.ImplicitUsings)
                allTrees.Add(CSharpSyntaxTree.ParseText(
                    "global using System; global using System.Collections.Generic; global using System.IO; " +
                    "global using System.Linq; global using System.Net.Http; global using System.Threading; " +
                    "global using System.Threading.Tasks;",
                    new CSharpParseOptions(config.LanguageVersion), path: "<narrant-implicit-usings>"));
            var output = config.SourcePaths[0] == "narrant.csharp.json" &&
                trees.Values.Any(tree => tree.GetRoot(cancellation) is CompilationUnitSyntax unit &&
                    unit.Members.OfType<GlobalStatementSyntax>().Any())
                ? OutputKind.ConsoleApplication : config.Output;
            var compilation = CSharpCompilation.Create(config.AssemblyName, allTrees, references,
                new CSharpCompilationOptions(output, nullableContextOptions: config.Nullable));
            compiling.Remove(config.Id);
            return compiled[config.Id] = (compilation, trees, platform);
        }
    }

    /// <summary>仅加载目标平台明确批准的 reference DLL，不读取宿主 runtime。</summary>
    private static MetadataReference[] References(Platform platform)
    {
        if (platform.Version != "10.0.0" || !Path.IsPathFullyQualified(platform.Root) ||
            !Directory.Exists(platform.Root) || platform.Fingerprint.Length == 0)
            throw new HelperException("target_unavailable", "Invalid .NET reference platform");
        var files = Directory.GetFiles(platform.Root, "*.dll", SearchOption.TopDirectoryOnly);
        if (files.Length < 10 || !files.Any(f => Path.GetFileName(f) == "System.Runtime.dll"))
            throw new HelperException("target_unavailable", "Incomplete .NET reference assembly directory");
        if (files.Any(file => File.GetAttributes(file).HasFlag(FileAttributes.ReparsePoint)))
            throw new HelperException("target_unavailable", "Reference assemblies must not be symbolic links");
        return files.Order(StringComparer.Ordinal).Select(file => MetadataReference.CreateFromFile(file)).ToArray();
    }
}

/// <summary>保留提取时的语义符号、来源以及选集标记。</summary>
internal sealed record ExtractedNode(GraphNode Node, string Path, bool Selected, ISymbol? Symbol,
    List<PendingReference> Pending);

/// <summary>在最终成员集确定前保留一个 Roslyn 符号引用。</summary>
internal sealed record PendingReference(string Semantic, ISymbol? Symbol, string? UnresolvedName);

/// <summary>合并同一原始文件在所有明确条件配置中出现的直接子声明跨度。</summary>
internal sealed class ProjectionIndex
{
    private readonly Dictionary<string, Dictionary<string, SyntaxNode>> children = new(StringComparer.Ordinal);

    /// <summary>跨配置收集相同词法 owner 的直接成员，保留所有分支原文。</summary>
    public ProjectionIndex(IEnumerable<SyntaxTree> trees)
    {
        foreach (var tree in trees) Visit(tree.GetRoot());
    }

    /// <summary>返回 owner 的全部分支子声明，按物理源码位置排序。</summary>
    public SyntaxNode[] Children(SyntaxNode owner) => children.TryGetValue(Key(owner), out var items)
        ? items.Values.OrderBy(node => node.SpanStart).ToArray() : [];

    /// <summary>递归收集文件、namespace、type 的直接子声明。</summary>
    private void Visit(SyntaxNode owner)
    {
        IEnumerable<SyntaxNode> members = owner switch
        {
            CompilationUnitSyntax unit => unit.Members,
            BaseNamespaceDeclarationSyntax ns => ns.Members,
            ExtensionBlockDeclarationSyntax extension => extension.Members,
            TypeDeclarationSyntax type => type.Members,
            EnumDeclarationSyntax enumeration => enumeration.Members,
            _ => []
        };
        var list = members.ToArray();
        if (list.Length == 0) return;
        if (!children.TryGetValue(Key(owner), out var map)) children[Key(owner)] = map = new(StringComparer.Ordinal);
        foreach (var member in list)
        {
            map.TryAdd(member.SpanStart + ":" + member.Span.Length, member);
            Visit(member);
        }
    }

    /// <summary>以固定侧路径和原文跨度定位跨配置共用的 owner。</summary>
    private static string Key(SyntaxNode owner) => owner.SyntaxTree.FilePath + "\0" + owner.Kind() + ":" +
        owner.SpanStart + ":" + owner.Span.Length;
}

/// <summary>将 Roslyn 声明和表达式映射为带源码身份的图对象。</summary>
internal static class Extractor
{
    /// <summary>遍历一棵编译树，创建文件及其明确声明对象。</summary>
    public static void Extract(string path, SyntaxTree tree, SemanticModel model, ProjectionIndex projection, bool selected,
        List<ExtractedNode> output, CancellationToken cancellation)
    {
        var root = (CompilationUnitSyntax)tree.GetRoot(cancellation);
        var fileId = "csharp:" + Escape(path) + "#file";
        var file = New("container", fileId, "file", Path.GetFileName(path), null,
            Project(root.ToFullString(), projection.Children(root).Select(member => member.Span)), root.ToFullString());
        output.Add(new ExtractedNode(file, path, selected, null, []));
        foreach (var member in root.Members)
            Visit(member, file, path, tree, model, projection, selected, output, cancellation);
        var globals = root.Members.OfType<GlobalStatementSyntax>().ToArray();
        if (globals.Length > 0)
        {
            var source = string.Join("\n", globals.Select(g => g.ToFullString()));
            var node = New("execution_region", fileId + "/top-level", "top_level_statements", null,
                "top-level", Normalize(source), source);
            file.Contains.Add(node.Id);
            var pending = References(globals.Cast<SyntaxNode>(), model, cancellation);
            output.Add(new ExtractedNode(node, path, selected, null, pending));
        }
    }

    /// <summary>按词法父级递归提取声明，局部表达式留在最近对象内。</summary>
    private static void Visit(SyntaxNode syntax, GraphNode parent, string path, SyntaxTree tree, SemanticModel model,
        ProjectionIndex projection,
        bool selected, List<ExtractedNode> output, CancellationToken cancellation)
    {
        cancellation.ThrowIfCancellationRequested();
        if (syntax is GlobalStatementSyntax) return;
        if (syntax is ExtensionBlockDeclarationSyntax extensionBlock)
        {
            var id = parent.Id + "/extension:" + Escape(Header(extensionBlock));
            var code = Project(extensionBlock.ToFullString(), projection.Children(extensionBlock)
                .Select(member => Shift(member.Span, extensionBlock.FullSpan.Start)));
            var node = New("extension", id, "extension_block", "extension", Header(extensionBlock),
                code, extensionBlock.ToFullString());
            parent.Contains.Add(node.Id);
            output.Add(new ExtractedNode(node, path, selected, model.GetDeclaredSymbol(extensionBlock, cancellation), []));
            foreach (var member in extensionBlock.Members)
                Visit(member, node, path, tree, model, projection, selected, output, cancellation);
            return;
        }
        if (syntax is NamespaceDeclarationSyntax or FileScopedNamespaceDeclarationSyntax)
        {
            var ns = (BaseNamespaceDeclarationSyntax)syntax;
            var id = parent.Id + "/namespace:" + Escape(ns.Name.ToString());
            var node = New("container", id, "namespace", ns.Name.ToString(), Normalize(ns.Name.ToString()),
                Project(ns.ToFullString(), projection.Children(ns).Select(member => Shift(member.Span, ns.FullSpan.Start))), ns.ToFullString());
            parent.Contains.Add(node.Id);
            output.Add(new ExtractedNode(node, path, selected, model.GetDeclaredSymbol(ns, cancellation), []));
            foreach (var member in ns.Members) Visit(member, node, path, tree, model, projection, selected, output, cancellation);
            return;
        }
        if (syntax is BaseTypeDeclarationSyntax or DelegateDeclarationSyntax)
        {
            var symbol = model.GetDeclaredSymbol(syntax, cancellation);
            var name = syntax switch
            {
                BaseTypeDeclarationSyntax value => value.Identifier.ValueText,
                DelegateDeclarationSyntax value => value.Identifier.ValueText,
                _ => "type"
            };
            var kind = syntax.Kind().ToString().Replace("Declaration", "", StringComparison.Ordinal).ToLowerInvariant();
            var arity = symbol is INamedTypeSymbol named ? named.Arity : 0;
            var branch = BranchMarker(tree, syntax.SpanStart);
            var id = parent.Id + "/" + kind + ":" + Escape(name) + "`" + arity + branch;
            var children = syntax is TypeDeclarationSyntax type ? type.Members.Cast<SyntaxNode>().ToArray() :
                syntax is EnumDeclarationSyntax enumeration ? enumeration.Members.Cast<SyntaxNode>().ToArray() : [];
            var allChildren = projection.Children(syntax);
            var orderSensitive = allChildren.Where(child => child is FieldDeclarationSyntax or EventFieldDeclarationSyntax or
                PropertyDeclarationSyntax or EnumMemberDeclarationSyntax).Select(child => child switch
                {
                    BaseFieldDeclarationSyntax field => string.Join(",", field.Declaration.Variables.Select(v => v.Identifier.ValueText)),
                    PropertyDeclarationSyntax property => property.Identifier.ValueText,
                    EnumMemberDeclarationSyntax member => member.Identifier.ValueText,
                    _ => ""
                }).ToArray();
            var code = Project(syntax.ToFullString(), allChildren.Select(child => Shift(child.Span, syntax.FullSpan.Start))) +
                (orderSensitive.Length > 0 ? "|order:" + string.Join(",", orderSensitive) : "");
            var node = New("type", id, kind, name, Header(syntax), code, syntax.ToFullString());
            parent.Contains.Add(node.Id);
            var pending = new List<PendingReference>();
            if (syntax is BaseTypeDeclarationSyntax baseType && baseType.BaseList is not null)
                foreach (var baseSyntax in baseType.BaseList.Types)
                {
                    var baseSymbol = model.GetTypeInfo(baseSyntax.Type, cancellation).Type;
                    pending.Add(new PendingReference(baseSymbol?.TypeKind == TypeKind.Interface ? "code.implements" : "code.extends",
                        baseSymbol, baseSymbol is null ? baseSyntax.Type.ToString() : null));
                }
            output.Add(new ExtractedNode(node, path, selected, symbol, pending));
            foreach (var child in children) Visit(child, node, path, tree, model, projection, selected, output, cancellation);
            return;
        }
        if (syntax is FieldDeclarationSyntax field)
        {
            foreach (var variable in field.Declaration.Variables)
                VisitVariable(variable, field, parent, path, selected, model, output, cancellation);
            return;
        }
        if (syntax is EventFieldDeclarationSyntax eventField)
        {
            foreach (var variable in eventField.Declaration.Variables)
                VisitVariable(variable, eventField, parent, path, selected, model, output, cancellation);
            return;
        }
        if (syntax is EnumMemberDeclarationSyntax enumMember)
        {
            var code = enumMember.EqualsValue is null ? "implicit-order:" +
                ((EnumDeclarationSyntax)enumMember.Parent!).Members.IndexOf(enumMember) : Normalize(enumMember.EqualsValue.ToFullString());
            AddLeaf("value", "enum_member", enumMember.Identifier.ValueText, enumMember, parent, path, selected,
                model.GetDeclaredSymbol(enumMember, cancellation), code, output, model, cancellation);
            return;
        }
        if (syntax is PropertyDeclarationSyntax property)
        {
            var node = AddLeaf("value", "property", property.Identifier.ValueText, property, parent, path, selected,
                model.GetDeclaredSymbol(property, cancellation), property.Initializer is null ? null : Normalize(property.Initializer.ToFullString()),
                output, model, cancellation);
            Accessors(property.AccessorList, property.ExpressionBody, node, path, selected, model, output, cancellation);
            return;
        }
        if (syntax is IndexerDeclarationSyntax indexer)
        {
            var node = AddLeaf("value", "indexer", "this", indexer, parent, path, selected,
                model.GetDeclaredSymbol(indexer, cancellation), null, output, model, cancellation);
            Accessors(indexer.AccessorList, indexer.ExpressionBody, node, path, selected, model, output, cancellation);
            return;
        }
        if (syntax is EventDeclarationSyntax eventDeclaration)
        {
            var node = AddLeaf("value", "event", eventDeclaration.Identifier.ValueText, eventDeclaration, parent,
                path, selected, model.GetDeclaredSymbol(eventDeclaration, cancellation), null, output, model, cancellation);
            Accessors(eventDeclaration.AccessorList, null, node, path, selected, model, output, cancellation);
            return;
        }
        if (syntax is BaseMethodDeclarationSyntax method)
        {
            var name = method switch
            {
                MethodDeclarationSyntax m => m.Identifier.ValueText,
                ConstructorDeclarationSyntax m => m.Identifier.ValueText,
                DestructorDeclarationSyntax m => "~" + m.Identifier.ValueText,
                OperatorDeclarationSyntax m => "operator " + m.OperatorToken.Text,
                ConversionOperatorDeclarationSyntax m => m.ImplicitOrExplicitKeyword.Text + " operator " + m.Type,
                _ => "method"
            };
            var kind = method.Kind().ToString().Replace("Declaration", "", StringComparison.Ordinal).ToLowerInvariant();
            var symbol = model.GetDeclaredSymbol(method, cancellation);
            AddLeaf("callable", kind, name, method, parent, path, selected, symbol,
                Body(method), output, model, cancellation);
            return;
        }
        throw new HelperException("object_extraction_failed", $"Unsupported C# declaration {syntax.Kind()}", path);
    }

    /// <summary>按单个变量声明提取 field 或 field-like event。</summary>
    private static void VisitVariable(VariableDeclaratorSyntax variable, MemberDeclarationSyntax declaration, GraphNode parent,
        string path, bool selected, SemanticModel model, List<ExtractedNode> output, CancellationToken cancellation)
    {
        var kind = declaration is EventFieldDeclarationSyntax ? "event" : "field";
        AddLeaf("value", kind, variable.Identifier.ValueText, variable, parent, path, selected,
            model.GetDeclaredSymbol(variable, cancellation), variable.Initializer is null ? null : Normalize(variable.Initializer.ToFullString()),
            output, model, cancellation);
    }

    /// <summary>提取显式访问器；属性表达式体形成一个有源码的 getter。</summary>
    private static void Accessors(AccessorListSyntax? list, ArrowExpressionClauseSyntax? expression, GraphNode parent,
        string path, bool selected, SemanticModel model, List<ExtractedNode> output, CancellationToken cancellation)
    {
        if (list is not null)
            foreach (var accessor in list.Accessors)
                AddLeaf("callable", accessor.Keyword.ValueText, accessor.Keyword.ValueText, accessor, parent,
                    path, selected, model.GetDeclaredSymbol(accessor, cancellation),
                    accessor.Body is not null ? Normalize(accessor.Body.ToFullString()) :
                    accessor.ExpressionBody is not null ? Normalize(accessor.ExpressionBody.ToFullString()) : null,
                    output, model, cancellation);
        if (expression is not null)
        {
            var node = New("callable", parent.Id + "/get", "get", "get", "get", Normalize(expression.ToFullString()),
                expression.ToFullString());
            parent.Contains.Add(node.Id);
            output.Add(new ExtractedNode(node, path, selected, null, References([expression], model, cancellation)));
        }
    }

    /// <summary>为一个有独立源码的成员建立稳定槽位并记录其引用。</summary>
    private static GraphNode AddLeaf(string category, string kind, string name, SyntaxNode syntax, GraphNode parent,
        string path, bool selected, ISymbol? symbol, string? code, List<ExtractedNode> output,
        SemanticModel model, CancellationToken cancellation)
    {
        var parameters = syntax switch
        {
            BaseMethodDeclarationSyntax method => string.Join(",", method.ParameterList.Parameters.Select(ParameterSlot)),
            IndexerDeclarationSyntax indexer => string.Join(",", indexer.ParameterList.Parameters.Select(ParameterSlot)),
            _ => ""
        };
        var arity = symbol is IMethodSymbol m ? m.Arity : 0;
        var slot = kind + ":" + name + "`" + arity + "(" + parameters + ")";
        var role = symbol is IMethodSymbol partial ? partial.PartialImplementationPart is not null ? ":definition" :
            partial.PartialDefinitionPart is not null ? ":implementation" : "" : "";
        var branch = BranchMarker(syntax.SyntaxTree, syntax.SpanStart);
        var id = parent.Id + "/" + Escape(slot + role) + branch;
        var signature = Header(syntax);
        var node = New(category, id, kind, name, signature, code, syntax.ToFullString());
        parent.Contains.Add(id);
        var scope = syntax switch
        {
            BaseMethodDeclarationSyntax method when method.Body is not null => (SyntaxNode?)method.Body,
            BaseMethodDeclarationSyntax method when method.ExpressionBody is not null => method.ExpressionBody,
            AccessorDeclarationSyntax accessor when accessor.Body is not null => accessor.Body,
            AccessorDeclarationSyntax accessor when accessor.ExpressionBody is not null => accessor.ExpressionBody,
            VariableDeclaratorSyntax variable => variable.Initializer,
            PropertyDeclarationSyntax property => property.Initializer,
            EnumMemberDeclarationSyntax member => member.EqualsValue,
            _ => null
        };
        var references = scope is null ? [] : References([scope], model, cancellation);
        if (symbol?.ContainingType is { } owner) references.Add(new PendingReference("code.member_of", owner, null));
        if (symbol is IFieldSymbol field) references.Add(new PendingReference("code.type_use", field.Type, null));
        if (symbol is IPropertySymbol prop) references.Add(new PendingReference("code.type_use", prop.Type, null));
        if (symbol is IMethodSymbol implementation && implementation.ContainingType is { } containing)
        {
            if (implementation.OverriddenMethod is { } overridden)
                references.Add(new PendingReference("code.extends", overridden, null));
            foreach (var contract in containing.AllInterfaces.SelectMany(iface => iface.GetMembers()))
                if (SymbolEqualityComparer.Default.Equals(containing.FindImplementationForInterfaceMember(contract), implementation))
                    references.Add(new PendingReference("code.implements", contract, null));
        }
        output.Add(new ExtractedNode(node, path, selected, symbol, references));
        return node;
    }

    /// <summary>仅用源码参数类型与传递方式构造重载槽，不纳入参数名。</summary>
    private static string ParameterSlot(ParameterSyntax parameter)
    {
        var passing = string.Join(" ", parameter.Modifiers.Select(m => m.ValueText));
        return passing + ":" + Normalize(parameter.Type?.ToFullString() ?? "");
    }

    /// <summary>收集显式调用、构造与方法组；无法确认的调用保留观测名称。</summary>
    private static List<PendingReference> References(IEnumerable<SyntaxNode> roots, SemanticModel model, CancellationToken cancellation)
    {
        var result = new List<PendingReference>();
        foreach (var root in roots)
        {
            foreach (var invocation in root.DescendantNodesAndSelf().OfType<InvocationExpressionSyntax>())
            {
                cancellation.ThrowIfCancellationRequested();
                var symbol = model.GetSymbolInfo(invocation, cancellation).Symbol;
                result.Add(new PendingReference("code.calls", symbol is IMethodSymbol ? symbol : null,
                    symbol is IMethodSymbol ? null : invocation.Expression.ToString()));
            }
            foreach (var creation in root.DescendantNodesAndSelf().OfType<ObjectCreationExpressionSyntax>())
            {
                var symbol = model.GetSymbolInfo(creation, cancellation).Symbol;
                result.Add(new PendingReference("code.calls", symbol is IMethodSymbol ? symbol : null,
                    symbol is IMethodSymbol ? null : creation.Type.ToString()));
            }
            foreach (var creation in root.DescendantNodesAndSelf().OfType<ImplicitObjectCreationExpressionSyntax>())
            {
                var symbol = model.GetSymbolInfo(creation, cancellation).Symbol;
                result.Add(new PendingReference("code.calls", symbol is IMethodSymbol ? symbol : null,
                    symbol is IMethodSymbol ? null : creation.ToString()));
            }
            foreach (var initializer in root.DescendantNodesAndSelf().OfType<ConstructorInitializerSyntax>())
            {
                var symbol = model.GetSymbolInfo(initializer, cancellation).Symbol;
                result.Add(new PendingReference("code.calls", symbol is IMethodSymbol ? symbol : null,
                    symbol is IMethodSymbol ? null : initializer.ToString()));
            }
            foreach (var expression in root.DescendantNodesAndSelf().OfType<ExpressionSyntax>())
            {
                if (expression is not (IdentifierNameSyntax or MemberAccessExpressionSyntax)) continue;
                if (expression.Parent is InvocationExpressionSyntax or MemberAccessExpressionSyntax) continue;
                var symbol = model.GetSymbolInfo(expression, cancellation).Symbol;
                if (symbol is IMethodSymbol method)
                    result.Add(new PendingReference("code.references", method, null));
            }
        }
        return result;
    }

    /// <summary>从声明头剥离函数体与独立成员的实现。</summary>
    private static string Header(SyntaxNode syntax)
    {
        var end = syntax switch
        {
            BaseMethodDeclarationSyntax method when method.Body is not null => method.Body.SpanStart,
            BaseMethodDeclarationSyntax method when method.ExpressionBody is not null => method.ExpressionBody.SpanStart,
            AccessorDeclarationSyntax accessor when accessor.Body is not null => accessor.Body.SpanStart,
            AccessorDeclarationSyntax accessor when accessor.ExpressionBody is not null => accessor.ExpressionBody.SpanStart,
            TypeDeclarationSyntax type when type.OpenBraceToken.IsKind(SyntaxKind.OpenBraceToken) => type.OpenBraceToken.SpanStart,
            EnumDeclarationSyntax type => type.OpenBraceToken.SpanStart,
            PropertyDeclarationSyntax property when property.AccessorList is not null => property.AccessorList.SpanStart,
            PropertyDeclarationSyntax property when property.ExpressionBody is not null => property.ExpressionBody.SpanStart,
            IndexerDeclarationSyntax indexer when indexer.AccessorList is not null => indexer.AccessorList.SpanStart,
            IndexerDeclarationSyntax indexer when indexer.ExpressionBody is not null => indexer.ExpressionBody.SpanStart,
            EventDeclarationSyntax evt when evt.AccessorList is not null => evt.AccessorList.SpanStart,
            _ => syntax.Span.End
        };
        var text = syntax.SyntaxTree.GetText().ToString(Microsoft.CodeAnalysis.Text.TextSpan.FromBounds(syntax.SpanStart, end));
        return Normalize(text);
    }

    /// <summary>提取显式方法体，声明而无实现时返回 null。</summary>
    private static string? Body(BaseMethodDeclarationSyntax method) => method.Body is not null
        ? Normalize(method.Body.ToFullString()) : method.ExpressionBody is not null
            ? Normalize(method.ExpressionBody.ToFullString()) : null;

    /// <summary>用相对路径和预处理分支字面条件标识同名条件声明。</summary>
    private static string BranchMarker(SyntaxTree tree, int offset)
    {
        var text = tree.GetText().ToString(new Microsoft.CodeAnalysis.Text.TextSpan(0, offset));
        var stack = new List<string>();
        foreach (var line in text.Split('\n'))
        {
            var trimmed = line.TrimStart();
            if (trimmed.StartsWith("#if ", StringComparison.Ordinal)) stack.Add(trimmed);
            else if (trimmed.StartsWith("#elif ", StringComparison.Ordinal) && stack.Count > 0) stack[^1] = trimmed;
            else if (trimmed.StartsWith("#else", StringComparison.Ordinal) && stack.Count > 0) stack[^1] += ":else";
            else if (trimmed.StartsWith("#endif", StringComparison.Ordinal) && stack.Count > 0) stack.RemoveAt(stack.Count - 1);
        }
        if (stack.Count == 0) return "";
        var hash = SHA256.HashData(Encoding.UTF8.GetBytes(string.Join("/", stack)));
        return "~" + Convert.ToHexString(hash[..6]).ToLowerInvariant();
    }

    /// <summary>把子对象源码换成固定锚点，父内容不重复包含子正文。</summary>
    private static string Project(string source, IEnumerable<Microsoft.CodeAnalysis.Text.TextSpan> excluded)
    {
        var spans = excluded.OrderBy(span => span.Start).ToArray();
        var buffer = new StringBuilder();
        var cursor = 0;
        foreach (var span in spans)
        {
            if (span.Start < cursor || span.End > source.Length) continue;
            buffer.Append(source, cursor, span.Start - cursor);
            buffer.Append(" <child> ");
            cursor = span.End;
        }
        buffer.Append(source, cursor, source.Length - cursor);
        return Normalize(buffer.ToString());
    }

    /// <summary>把绝对 SyntaxSpan 转成当前对象原文的相对跨度。</summary>
    private static Microsoft.CodeAnalysis.Text.TextSpan Shift(Microsoft.CodeAnalysis.Text.TextSpan span, int offset)
        => new(span.Start - offset, span.Length);

    /// <summary>创建全字段图节点。</summary>
    private static GraphNode New(string category, string id, string kind, string? name,
        string? signature, string? code, string? source) => new()
    {
        Category = category, Id = id, NativeKind = kind, Name = name,
        Signature = signature, Code = code, Source = source
    };

    /// <summary>对身份组件做确定性百分号转义。</summary>
    private static string Escape(string value) => Uri.EscapeDataString(value);

    /// <summary>去除普通注释与排版，同时保存字面量及预处理文字。</summary>
    internal static string Normalize(string text)
    {
        var builder = new StringBuilder(text.Length);
        var pendingSpace = false;
        for (var i = 0; i < text.Length;)
        {
            var c = text[i];
            if (char.IsWhiteSpace(c)) { pendingSpace = true; i++; continue; }
            if (c == '/' && i + 1 < text.Length && text[i + 1] == '/')
            {
                i += 2;
                while (i < text.Length && text[i] is not ('\n' or '\r')) i++;
                pendingSpace = true;
                continue;
            }
            if (c == '/' && i + 1 < text.Length && text[i + 1] == '*')
            {
                i += 2;
                while (i + 1 < text.Length && !(text[i] == '*' && text[i + 1] == '/')) i++;
                i = Math.Min(i + 2, text.Length);
                pendingSpace = true;
                continue;
            }
            if (pendingSpace && builder.Length > 0 && (char.IsLetterOrDigit(builder[^1]) || builder[^1] == '_') &&
                (char.IsLetterOrDigit(c) || c == '_')) builder.Append(' ');
            pendingSpace = false;
            if (c is '"' or '\'' || (c == '@' && i + 1 < text.Length && text[i + 1] == '"'))
            {
                var verbatim = c == '@';
                if (verbatim) { builder.Append('@'); i++; c = text[i]; }
                var quote = c;
                var raw = quote == '"' && i + 2 < text.Length && text[i + 1] == '"' && text[i + 2] == '"';
                var count = raw ? 3 : 1;
                for (var n = 0; n < count; n++) builder.Append(text[i++]);
                while (i < text.Length)
                {
                    var next = text[i++];
                    builder.Append(next);
                    if (!verbatim && !raw && next == '\\' && i < text.Length) { builder.Append(text[i++]); continue; }
                    if (verbatim && next == '"' && i < text.Length && text[i] == '"') { builder.Append(text[i++]); continue; }
                    if (!raw && next == quote) break;
                    if (raw && next == '"' && i + 1 < text.Length && text[i] == '"' && text[i + 1] == '"')
                    { builder.Append(text[i++]); builder.Append(text[i++]); break; }
                }
                continue;
            }
            builder.Append(c);
            i++;
        }
        return builder.ToString();
    }
}
