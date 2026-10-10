using System.Text.Json;

namespace Narrant.CSharp.Helper;

/// <summary>封装协议输入错误及明确的失败代码。</summary>
internal sealed class HelperException(string code, string message, params string[] paths) : Exception(message)
{
    public string Code { get; } = code;
    public string[] Paths { get; } = paths;
}

/// <summary>保存一次分析请求及其严格校验后的字段。</summary>
internal sealed record AnalyzeRequest(string Id, Input Input, EnvironmentInput Environment, Limits Limits);

/// <summary>保存固定侧仓库及选集。</summary>
internal sealed record Input(string RepositoryRoot, string[] Files, bool IncludeTests, string Profile);

/// <summary>保存宿主准许读取的目标平台与 scratch。</summary>
internal sealed record EnvironmentInput(string ScratchRoot, Platform[] Platforms);

/// <summary>描述一个单独批准的目标引用程序集目录。</summary>
internal sealed record Platform(string Id, string Kind, string Version, string Root, PlatformMetadata Metadata);

/// <summary>定义 C# 平台的有限 metadata 字段。</summary>
internal sealed record PlatformMetadata(string Tfm, string ReferencePack, string ReferencePackVersion);

/// <summary>保存本次请求的三个资源上限。</summary>
internal sealed record Limits(int AnalysisTimeoutMs, long MaxResultBytes, int MaxDiagnosticBytes);

/// <summary>按 v1 协议读取消息，拒绝重复字段、缺字段和未知字段。</summary>
internal static class Protocol
{
    public const string Version = "narrant.native-analyzer/1";
    public static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        DictionaryKeyPolicy = JsonNamingPolicy.SnakeCaseLower
    };

    /// <summary>解析唯一的 analyze 请求并验证嵌套对象形状。</summary>
    public static AnalyzeRequest ParseAnalyze(string line)
    {
        using var document = JsonDocument.Parse(line, new JsonDocumentOptions { MaxDepth = 32 });
        var root = document.RootElement;
        Fields(root, "protocol", "type", "id", "input", "environment", "limits");
        if (String(root, "protocol") != Version || String(root, "type") != "analyze")
            throw new HelperException("invalid_input", "Expected v1 analyze message");
        var id = String(root, "id");
        if (id.Length == 0) throw new HelperException("invalid_input", "Empty request id");
        var input = root.GetProperty("input");
        Fields(input, "repository_root", "files", "include_tests", "profile");
        var environment = root.GetProperty("environment");
        Fields(environment, "scratch_root", "platforms");
        var limits = root.GetProperty("limits");
        Fields(limits, "analysis_timeout_ms", "max_result_bytes", "max_diagnostic_bytes");
        var platforms = new List<Platform>();
        foreach (var value in environment.GetProperty("platforms").EnumerateArray())
        {
            Fields(value, "id", "kind", "version", "root", "metadata");
            var metadata = value.GetProperty("metadata");
            Fields(metadata, "tfm", "reference_pack", "reference_pack_version");
            platforms.Add(new Platform(String(value, "id"), String(value, "kind"), String(value, "version"),
                String(value, "root"),
                new PlatformMetadata(String(metadata, "tfm"), String(metadata, "reference_pack"),
                    String(metadata, "reference_pack_version"))));
        }
        var request = new AnalyzeRequest(id,
            new Input(String(input, "repository_root"), Strings(input, "files"),
                input.GetProperty("include_tests").GetBoolean(), String(input, "profile")),
            new EnvironmentInput(String(environment, "scratch_root"), platforms.ToArray()),
            new Limits(limits.GetProperty("analysis_timeout_ms").GetInt32(),
                limits.GetProperty("max_result_bytes").GetInt64(),
                limits.GetProperty("max_diagnostic_bytes").GetInt32()));
        if (request.Limits.AnalysisTimeoutMs <= 0 || request.Limits.MaxResultBytes <= 0 ||
            request.Limits.MaxDiagnosticBytes <= 0)
            throw new HelperException("invalid_input", "Limits must be positive");
        return request;
    }

    /// <summary>读取取消消息，必须只包含约定字段。</summary>
    public static string ParseCancel(string line)
    {
        using var document = JsonDocument.Parse(line);
        Fields(document.RootElement, "protocol", "type", "id");
        if (String(document.RootElement, "protocol") != Version || String(document.RootElement, "type") != "cancel")
            throw new HelperException("invalid_input", "Expected v1 cancel message");
        return String(document.RootElement, "id");
    }

    /// <summary>序列化协议响应。</summary>
    public static string Serialize(object value) => JsonSerializer.Serialize(value, Json);

    /// <summary>验证 JSON 对象字段集合并拒绝重复键。</summary>
    private static void Fields(JsonElement element, params string[] expected)
    {
        if (element.ValueKind != JsonValueKind.Object) throw new HelperException("invalid_input", "Expected JSON object");
        var found = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in element.EnumerateObject())
            if (!expected.Contains(property.Name, StringComparer.Ordinal) || !found.Add(property.Name))
                throw new HelperException("invalid_input", $"Unknown or duplicate field: {property.Name}");
        if (found.Count != expected.Length) throw new HelperException("invalid_input", "Missing required field");
    }

    /// <summary>读取非空 JSON 字符串。</summary>
    private static string String(JsonElement element, string name)
    {
        var value = element.GetProperty(name);
        if (value.ValueKind != JsonValueKind.String || string.IsNullOrEmpty(value.GetString()))
            throw new HelperException("invalid_input", $"Expected nonempty string: {name}");
        return value.GetString()!;
    }

    /// <summary>读取字符串数组并拒绝重复项。</summary>
    private static string[] Strings(JsonElement element, string name)
    {
        var values = element.GetProperty(name).EnumerateArray().Select(value => value.GetString()
            ?? throw new HelperException("invalid_input", $"Invalid {name}")).ToArray();
        if (values.Distinct(StringComparer.Ordinal).Count() != values.Length)
            throw new HelperException("invalid_input", $"Duplicate {name}");
        return values;
    }
}

/// <summary>描述最终图节点。</summary>
internal sealed class GraphNode
{
    public required string Category { get; init; }
    public required string Id { get; init; }
    public string Language { get; init; } = "csharp";
    public required string NativeKind { get; init; }
    public string? Name { get; init; }
    public string? Signature { get; init; }
    public string? Code { get; init; }
    public string? Source { get; init; }
    public List<string> Contains { get; } = [];
    public List<GraphReference> References { get; } = [];
    public Dictionary<string, object> Extensions { get; } = [];
}

/// <summary>描述语义关系及最终目标分类。</summary>
internal sealed record GraphReference(string Semantic, object Target);

/// <summary>描述图内、外部或无法解析的目标。</summary>
internal sealed record GraphTarget(string Kind, string? Id = null, string? Symbol = null, string? Name = null);

/// <summary>保存来源和覆盖证据。</summary>
internal sealed record Evidence(List<ConfigurationEvidence> Configurations, List<FileEvidence> Files, List<OriginEvidence> Origins);

/// <summary>记录实际使用的有限静态配置。</summary>
internal sealed record ConfigurationEvidence(string Id, string[] SourcePaths, string[] PlatformIds);

/// <summary>记录输入文件对应的分析状态。</summary>
internal sealed record FileEvidence(string Path, string Disposition, string[] ConfigurationIds);

/// <summary>记录图节点对应的选中原始源码。</summary>
internal sealed record OriginEvidence(string NodeId, string[] SelectedPaths);
