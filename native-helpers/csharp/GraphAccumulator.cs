using Microsoft.CodeAnalysis;

namespace Narrant.CSharp.Helper;

/// <summary>记录一个已确认的图外符号及其来源分类。</summary>
internal sealed record ExternalSymbol(string Symbol, string Name, string Scope);

/// <summary>验证多配置对象兼容性并在最终成员集上分类引用。</summary>
internal sealed class GraphAccumulator
{
    private readonly Dictionary<string, GraphNode> nodes = new(StringComparer.Ordinal);
    private readonly Dictionary<string, HashSet<string>> origins = new(StringComparer.Ordinal);
    private readonly Dictionary<string, ExternalSymbol> external = new(StringComparer.Ordinal);
    private readonly Dictionary<string, HashSet<string>> sourceNodes = new(StringComparer.Ordinal);
    private readonly List<(string Owner, PendingReference Reference)> pending = [];

    /// <summary>归并一个完整 Compilation 的提取结果，保留每次确证的关系。</summary>
    public void Merge(List<ExtractedNode> extracted, CancellationToken cancellation)
    {
        foreach (var item in extracted.Where(n => n.Selected))
        {
            cancellation.ThrowIfCancellationRequested();
            if (nodes.TryGetValue(item.Node.Id, out var old))
            {
                if (old.Category != item.Node.Category || old.NativeKind != item.Node.NativeKind ||
                    old.Name != item.Node.Name || old.Signature != item.Node.Signature || old.Code != item.Node.Code ||
                    old.Source != item.Node.Source)
                    throw new HelperException("union_conflict", $"Conflicting C# object {item.Node.Id}", item.Path);
                foreach (var child in item.Node.Contains)
                    if (!old.Contains.Contains(child, StringComparer.Ordinal)) old.Contains.Add(child);
            }
            else nodes.Add(item.Node.Id, item.Node);
            if (!origins.TryGetValue(item.Node.Id, out var paths)) origins[item.Node.Id] = paths = new(StringComparer.Ordinal);
            paths.Add(item.Path);
            if (item.Symbol is not null)
                foreach (var syntax in item.Symbol.DeclaringSyntaxReferences.Where(s => s.SyntaxTree.FilePath == item.Path))
                {
                    var key = LocationKey(syntax.SyntaxTree.FilePath, syntax.Span.Start, syntax.Span.Length);
                    if (!sourceNodes.TryGetValue(key, out var ids)) sourceNodes[key] = ids = new(StringComparer.Ordinal);
                    ids.Add(item.Node.Id);
                }
            foreach (var reference in item.Pending) pending.Add((item.Node.Id, reference));
        }
    }

    /// <summary>校验单父与可达性，输出稳定排序的最终 DTO 和来源记录。</summary>
    public AnalysisOutput Finish(Evidence evidence)
    {
        ResolveReferences();
        var children = new HashSet<string>(StringComparer.Ordinal);
        foreach (var node in nodes.Values)
        {
            node.Contains.Sort(StringComparer.Ordinal);
            foreach (var child in node.Contains)
            {
                if (!nodes.ContainsKey(child) || !children.Add(child))
                    throw new HelperException("union_conflict", $"Invalid or multiply owned child {child}");
            }
            node.References.Sort((a, b) => string.CompareOrdinal(Protocol.Serialize(a), Protocol.Serialize(b)));
        }
        var roots = nodes.Keys.Where(id => !children.Contains(id)).Order(StringComparer.Ordinal).ToArray();
        if (roots.Any(id => nodes[id].Category != "container"))
            throw new HelperException("union_conflict", "Orphan declaration in C# graph");
        var visited = new HashSet<string>(StringComparer.Ordinal);
        foreach (var root in roots) Walk(root);
        if (visited.Count != nodes.Count) throw new HelperException("union_conflict", "Containment cycle in C# graph");
        evidence.Origins.AddRange(nodes.Keys.Order(StringComparer.Ordinal).Select(id =>
            new OriginEvidence(id, origins[id].Order(StringComparer.Ordinal).ToArray())));
        var used = nodes.Values.SelectMany(node => node.References)
            .Select(reference => reference.Target)
            .OfType<Dictionary<string, string>>()
            .Where(target => target.GetValueOrDefault("kind") == "external")
            .Select(target => target["symbol"]).ToHashSet(StringComparer.Ordinal);
        var descriptors = external.Values.Where(e => used.Contains(e.Symbol)).OrderBy(e => e.Symbol, StringComparer.Ordinal).ToArray();
        return new AnalysisOutput(new
        {
            root_contains = roots, nodes = nodes.Values.OrderBy(n => n.Id, StringComparer.Ordinal).ToArray(),
            external_symbols = descriptors
        }, evidence);

        // 递归访问整个包含树，确保每个对象从一个根可达。
        void Walk(string id)
        {
            if (!visited.Add(id)) return;
            foreach (var child in nodes[id].Contains) Walk(child);
        }
    }

    /// <summary>用所有配置的最终源码成员集合解析待决引用。</summary>
    private void ResolveReferences()
    {
        foreach (var (ownerId, reference) in pending)
        {
            var owner = nodes[ownerId];
            if (reference.Symbol is null)
            {
                if (reference.UnresolvedName is not null)
                    AddReference(owner, new GraphReference(reference.Semantic,
                        new Dictionary<string, string> { ["kind"] = "unresolved", ["name"] = reference.UnresolvedName }));
                continue;
            }
            var symbol = reference.Symbol is IMethodSymbol method && method.ReducedFrom is not null
                ? method.ReducedFrom : reference.Symbol;
            if (symbol is IErrorTypeSymbol || symbol.ContainingType is IErrorTypeSymbol)
            {
                AddReference(owner, new GraphReference(reference.Semantic,
                    new Dictionary<string, string> { ["kind"] = "unresolved",
                        ["name"] = reference.UnresolvedName ?? symbol.Name }));
                continue;
            }
            var targets = new HashSet<string>(StringComparer.Ordinal);
            foreach (var syntax in symbol.OriginalDefinition.DeclaringSyntaxReferences)
            {
                var key = LocationKey(syntax.SyntaxTree.FilePath, syntax.Span.Start, syntax.Span.Length);
                if (sourceNodes.TryGetValue(key, out var ids)) targets.UnionWith(ids);
            }
            if (targets.Count > 0)
            {
                foreach (var target in targets.Order(StringComparer.Ordinal))
                    AddReference(owner, new GraphReference(reference.Semantic,
                        new Dictionary<string, string> { ["kind"] = "node", ["id"] = target }));
                continue;
            }
            var location = symbol.Locations.FirstOrDefault(l => l.IsInSource);
            var scope = location is null ? "dependency" : "repository";
            var name = symbol.ToDisplayString(SymbolDisplayFormat.FullyQualifiedFormat);
            var origin = location?.SourceTree?.FilePath;
            var identity = "csharp:" + (origin is null ?
                "metadata:" + symbol.ContainingAssembly?.Identity.Name : "source:" + origin) + "#" + name;
            if (external.TryGetValue(identity, out var existing) && (existing.Name != name || existing.Scope != scope))
                throw new HelperException("union_conflict", $"Conflicting external symbol {identity}");
            external[identity] = new ExternalSymbol(identity, name, scope);
            AddReference(owner, new GraphReference(reference.Semantic,
                new Dictionary<string, string> { ["kind"] = "external", ["symbol"] = identity }));
        }
    }

    /// <summary>构造仅在本次请求内使用的源码位置映射键。</summary>
    private static string LocationKey(string path, int start, int length) => path + "\0" + start + ":" + length;

    /// <summary>去掉同一次及跨配置的重复引用事实。</summary>
    private static void AddReference(GraphNode owner, GraphReference reference)
    {
        var serialized = Protocol.Serialize(reference);
        if (!owner.References.Any(existing => Protocol.Serialize(existing) == serialized)) owner.References.Add(reference);
    }
}
