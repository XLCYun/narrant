using System.Text.Json;
using System.Xml.Linq;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;

namespace Narrant.CSharp.Helper;

/// <summary>表示一个必须独立编译和分析的静态 C# 上下文。</summary>
internal sealed record StaticConfiguration(string Id, string[] Files, string Tfm, LanguageVersion LanguageVersion,
    string[] Defines, bool ImplicitUsings, string[] SourcePaths, string[] ProjectReferences, bool TestOnly,
    string[] InactiveFiles, string AssemblyName, NullableContextOptions Nullable, OutputKind Output,
    string ConfigurationName);

/// <summary>从受限声明式文件或普通 SDK 项目静态建立编译配置。</summary>
internal static class Configuration
{
    /// <summary>发现配置，并确保每个文件的归属由静态规则证明。</summary>
    public static List<StaticConfiguration> Discover(string root, CancellationToken cancellation)
    {
        foreach (var extension in new[] { ".razor", ".cshtml", ".asmdef" })
            if (SafeFiles(root, extension).Any())
                throw new HelperException("unsupported_configuration", $"Generated-source project model {extension} is unsupported");
        var sourceSet = Path.Combine(root, "narrant.csharp.json");
        var projects = SafeFiles(root, ".csproj").Order(StringComparer.Ordinal).ToArray();
        if (File.Exists(sourceSet))
        {
            CheckRepositoryPath(root, sourceSet);
            if (projects.Length > 0) throw new HelperException("unsupported_configuration", "Source-set and csproj models cannot be mixed");
            return ReadSourceSet(root, sourceSet);
        }
        if (projects.Length == 0) throw new HelperException("unsupported_configuration", "No narrant.csharp.json or supported csproj found");
        var output = new List<StaticConfiguration>();
        foreach (var project in projects)
        {
            cancellation.ThrowIfCancellationRequested();
            output.AddRange(ReadProject(root, project));
        }
        return output;
    }

    /// <summary>读取显式 source-set，所有编译成员必须逐一列出。</summary>
    private static List<StaticConfiguration> ReadSourceSet(string root, string path)
    {
        using var document = JsonDocument.Parse(File.ReadAllText(path));
        var objectRoot = document.RootElement;
        ExactFields(objectRoot, "version", "configurations");
        if (objectRoot.GetProperty("version").GetString() != "csharp-static-v1")
            throw new HelperException("unsupported_configuration", "Unsupported source-set version", "narrant.csharp.json");
        var output = new List<StaticConfiguration>();
        foreach (var config in objectRoot.GetProperty("configurations").EnumerateArray())
        {
            if (config.TryGetProperty("test_only", out _))
                ExactFields(config, "id", "files", "tfm", "language_version", "define_constants", "implicit_usings", "test_only");
            else ExactFields(config, "id", "files", "tfm", "language_version", "define_constants", "implicit_usings");
            var files = config.GetProperty("files").EnumerateArray().Select(v => v.GetString()!).ToArray();
            if (files.Distinct(StringComparer.Ordinal).Count() != files.Length)
                throw new HelperException("unsupported_configuration", "Duplicate source-set file", "narrant.csharp.json");
            foreach (var file in files) CheckFile(root, file);
            var id = config.GetProperty("id").GetString();
            if (string.IsNullOrWhiteSpace(id))
                throw new HelperException("unsupported_configuration", "Empty source-set id", "narrant.csharp.json");
            var lang = ParseLanguage(config.GetProperty("language_version").GetString()!);
            var defines = config.GetProperty("define_constants").EnumerateArray().Select(v => v.GetString()!).ToArray();
            if (defines.Any(value => string.IsNullOrWhiteSpace(value) ||
                !System.Text.RegularExpressions.Regex.IsMatch(value, "^[A-Za-z_][A-Za-z0-9_]*$")) ||
                defines.Distinct(StringComparer.Ordinal).Count() != defines.Length)
                throw new HelperException("unsupported_configuration", "Invalid or duplicate define constant", "narrant.csharp.json");
            output.Add(new StaticConfiguration(id, files,
                config.GetProperty("tfm").GetString()!, lang,
                defines,
                config.GetProperty("implicit_usings").GetBoolean(), ["narrant.csharp.json"], [],
                config.TryGetProperty("test_only", out var test) && test.GetBoolean(), [],
                id, NullableContextOptions.Disable, OutputKind.DynamicallyLinkedLibrary, id));
        }
        if (output.Count == 0 || output.Select(c => c.Id).Distinct(StringComparer.Ordinal).Count() != output.Count)
            throw new HelperException("unsupported_configuration", "Source-set has no configurations or duplicate ids");
        return output;
    }

    /// <summary>按固定子集读取普通 Microsoft.NET.Sdk 项目，不执行 MSBuild。</summary>
    private static List<StaticConfiguration> ReadProject(string root, string path)
    {
        var relative = Path.GetRelativePath(root, path).Replace('\\', '/');
        CheckRepositoryPath(root, path);
        for (var directory = Path.GetDirectoryName(path); directory is not null && directory.StartsWith(root, StringComparison.Ordinal);
             directory = Path.GetDirectoryName(directory))
            if (File.Exists(Path.Combine(directory, "Directory.Build.props")) ||
                File.Exists(Path.Combine(directory, "Directory.Build.targets")))
                throw new HelperException("unsupported_configuration", "Directory.Build imports require MSBuild evaluation", relative);
        XDocument document;
        try { document = XDocument.Load(path, LoadOptions.None); }
        catch (Exception error) { throw new HelperException("unsupported_configuration", error.Message, relative); }
        var project = document.Root;
        if (project?.Name.LocalName != "Project" || project.Attribute("Sdk")?.Value != "Microsoft.NET.Sdk" ||
            project.Attributes().Count() != 1)
            throw new HelperException("unsupported_configuration", "Only literal Microsoft.NET.Sdk projects are supported", relative);
        var properties = new Dictionary<string, string>(StringComparer.Ordinal);
        var compile = new List<(string operation, string value, string? groupCondition, string? condition)>();
        var projectReferences = new List<(string path, string? groupCondition, string? condition)>();
        foreach (var group in project.Elements())
        {
            var groupCondition = group.Attribute("Condition")?.Value;
            if (group.Attributes().Any(a => a.Name.LocalName != "Condition"))
                throw new HelperException("unsupported_configuration", "Conditional groups are not supported", relative);
            if (group.Name.LocalName == "PropertyGroup")
            {
                if (groupCondition is not null)
                    throw new HelperException("unsupported_configuration", "Conditional PropertyGroup is unsupported", relative);
                foreach (var prop in group.Elements())
                {
                    if (prop.Attributes().Any() || !new[] { "TargetFramework", "TargetFrameworks", "LangVersion", "DefineConstants",
                        "ImplicitUsings", "EnableDefaultCompileItems", "Nullable", "OutputType", "RootNamespace",
                        "AssemblyName", "TreatWarningsAsErrors", "IsTestProject", "Configuration", "Platform" }.Contains(prop.Name.LocalName))
                        throw new HelperException("unsupported_configuration", $"Unsupported property {prop.Name.LocalName}", relative);
                    if (properties.TryGetValue(prop.Name.LocalName, out var prior) && prior != prop.Value)
                        throw new HelperException("unsupported_configuration", $"Conflicting property {prop.Name.LocalName}", relative);
                    properties[prop.Name.LocalName] = prop.Value.Trim();
                }
            }
            else if (group.Name.LocalName == "ItemGroup")
            {
                foreach (var item in group.Elements())
                {
                    if (item.Name.LocalName == "ProjectReference" && item.Attribute("Include") is { } include &&
                        item.Attributes().All(a => a.Name.LocalName is "Include" or "Condition") && !item.Elements().Any())
                    {
                        if (include.Value.Contains('*') || include.Value.Contains('$'))
                            throw new HelperException("unsupported_configuration", "Dynamic ProjectReference is unsupported", relative);
                        var dependency = Path.GetRelativePath(root, Path.GetFullPath(Path.Combine(Path.GetDirectoryName(path)!, include.Value))).Replace('\\', '/');
                        if (dependency.StartsWith("../", StringComparison.Ordinal) || !dependency.EndsWith(".csproj", StringComparison.Ordinal) ||
                            !File.Exists(Path.Combine(root, dependency)))
                            throw new HelperException("unsupported_configuration", "ProjectReference is outside repository or missing", relative);
                        CheckRepositoryPath(root, Path.Combine(root, dependency));
                        projectReferences.Add((dependency, groupCondition, item.Attribute("Condition")?.Value));
                        continue;
                    }
                    if (item.Name.LocalName != "Compile" || item.Elements().Any())
                        throw new HelperException("unsupported_configuration", $"Unsupported item {item.Name.LocalName}", relative);
                    var attrs = item.Attributes().ToDictionary(a => a.Name.LocalName, a => a.Value);
                    var operation = attrs.ContainsKey("Include") ? "Include" : attrs.ContainsKey("Remove") ? "Remove" : "";
                    if (operation.Length == 0 || attrs.Keys.Except(new[] { operation, "Condition" }).Any())
                        throw new HelperException("unsupported_configuration", "Unsupported Compile attributes", relative);
                    compile.Add((operation, attrs[operation], groupCondition, attrs.GetValueOrDefault("Condition")));
                }
            }
            else throw new HelperException("unsupported_configuration", $"Unsupported project element {group.Name.LocalName}", relative);
        }
        if (properties.Values.Any(value => value.Contains("$(", StringComparison.Ordinal) || value.Contains("@(", StringComparison.Ordinal)))
            throw new HelperException("unsupported_configuration", "Property expansion is not supported", relative);
        var tfms = properties.GetValueOrDefault("TargetFrameworks")?.Split(';') ??
            [properties.GetValueOrDefault("TargetFramework") ?? ""];
        if (tfms.Any(string.IsNullOrWhiteSpace) || (properties.ContainsKey("TargetFramework") && properties.ContainsKey("TargetFrameworks")))
            throw new HelperException("unsupported_configuration", "A literal TFM or TFM list is required", relative);
        var projectDir = Path.GetDirectoryName(path)!;
        var defaultItemsText = properties.GetValueOrDefault("EnableDefaultCompileItems", "true");
        if (defaultItemsText is not ("true" or "false"))
            throw new HelperException("unsupported_configuration", "Invalid EnableDefaultCompileItems", relative);
        var defaultItems = defaultItemsText == "true";
        var implicitUsingsText = properties.GetValueOrDefault("ImplicitUsings", "disable");
        if (implicitUsingsText is not ("true" or "enable" or "false" or "disable"))
            throw new HelperException("unsupported_configuration", "Invalid ImplicitUsings", relative);
        var implicitUsings = implicitUsingsText is "true" or "enable";
        var defaultConfiguration = properties.GetValueOrDefault("Configuration", "Debug");
        var targetPlatform = properties.GetValueOrDefault("Platform", "AnyCPU");
        if (defaultConfiguration is not ("Debug" or "Release") || targetPlatform != "AnyCPU")
            throw new HelperException("unsupported_configuration", "Only Debug/Release and AnyCPU are supported", relative);
        var configurationVariants = new HashSet<string>(StringComparer.Ordinal) { defaultConfiguration };
        foreach (var condition in compile.SelectMany(item => new[] { item.groupCondition, item.condition })
            .Concat(projectReferences.SelectMany(item => new[] { item.groupCondition, item.condition })))
        {
            if (condition is null) continue;
            var match = ParseCondition(condition, relative);
            if (match.variable != "Configuration") continue;
            if (match.literal is not ("Debug" or "Release"))
                throw new HelperException("unsupported_configuration", "Unsupported Configuration literal", relative);
            configurationVariants.Add(match.literal);
        }
        var nullable = properties.GetValueOrDefault("Nullable", "disable") switch
        {
            "disable" => NullableContextOptions.Disable,
            "enable" => NullableContextOptions.Enable,
            "annotations" => NullableContextOptions.Annotations,
            "warnings" => NullableContextOptions.Warnings,
            _ => throw new HelperException("unsupported_configuration", "Unsupported Nullable setting", relative)
        };
        var outputKind = properties.GetValueOrDefault("OutputType", "Library") switch
        {
            "Library" => OutputKind.DynamicallyLinkedLibrary,
            "Exe" => OutputKind.ConsoleApplication,
            _ => throw new HelperException("unsupported_configuration", "Unsupported OutputType", relative)
        };
        var assemblyName = properties.GetValueOrDefault("AssemblyName", Path.GetFileNameWithoutExtension(path));
        var output = new List<StaticConfiguration>();
        foreach (var tfm in tfms)
        foreach (var configuration in configurationVariants.Order(StringComparer.Ordinal))
        {
            var conditionalCandidates = new HashSet<string>(StringComparer.Ordinal);
            var files = defaultItems
                ? SafeFiles(projectDir, ".cs", excludeHiddenDirectories: true)
                    .Select(file => Path.GetRelativePath(root, file).Replace('\\', '/'))
                    .ToHashSet(StringComparer.Ordinal)
                : new HashSet<string>(StringComparer.Ordinal);
            foreach (var item in compile)
            {
                var applies = ConditionMatches(item.groupCondition, tfm, configuration, targetPlatform, relative) &&
                    ConditionMatches(item.condition, tfm, configuration, targetPlatform, relative);
                foreach (var part in item.value.Split(';'))
                {
                    if (part.Contains('*') || part.Contains('?') || part.Contains("$(", StringComparison.Ordinal))
                        throw new HelperException("unsupported_configuration", "Compile glob or property expansion is unsupported", relative);
                    var file = Path.GetRelativePath(root, Path.GetFullPath(Path.Combine(projectDir, part))).Replace('\\', '/');
                    CheckFile(root, file);
                    if (item.operation == "Include") conditionalCandidates.Add(file);
                    if (!applies) continue;
                    if (item.operation == "Include") files.Add(file); else files.Remove(file);
                }
            }
            foreach (var file in files) CheckFile(root, file);
            var defines = new List<string> { "NET", "NETCOREAPP" };
            if (configuration == "Debug") defines.Add("DEBUG");
            defines.Add("TRACE");
            if (tfm == "net10.0") defines.AddRange(["NET10_0", "NET10_0_OR_GREATER", "NETCOREAPP1_0_OR_GREATER",
                "NETCOREAPP1_1_OR_GREATER", "NETCOREAPP2_0_OR_GREATER", "NETCOREAPP2_1_OR_GREATER",
                "NETCOREAPP2_2_OR_GREATER", "NETCOREAPP3_0_OR_GREATER", "NETCOREAPP3_1_OR_GREATER", "NET5_0_OR_GREATER",
                "NET6_0_OR_GREATER", "NET7_0_OR_GREATER", "NET8_0_OR_GREATER", "NET9_0_OR_GREATER"]);
            else throw new HelperException("unsupported_configuration", $"Unsupported TFM {tfm}", relative);
            defines.AddRange(properties.GetValueOrDefault("DefineConstants", "").Split(';', StringSplitOptions.RemoveEmptyEntries));
            output.Add(new StaticConfiguration(relative + ":" + tfm + ":" + configuration,
                files.Order(StringComparer.Ordinal).ToArray(), tfm,
                ParseLanguage(properties.GetValueOrDefault("LangVersion", "14.0")), defines.ToArray(),
                implicitUsings, [relative],
                projectReferences.Where(item =>
                    ConditionMatches(item.groupCondition, tfm, configuration, targetPlatform, relative) &&
                    ConditionMatches(item.condition, tfm, configuration, targetPlatform, relative))
                    .Select(item => item.path).ToArray(),
                properties.GetValueOrDefault("IsTestProject", "false") == "true",
                conditionalCandidates.Except(files).Order(StringComparer.Ordinal).ToArray(),
                assemblyName, nullable, outputKind, configuration));
        }
        return output;
    }

    /// <summary>只解释 TargetFramework、Configuration 和 Platform 的字面等值条件。</summary>
    private static bool ConditionMatches(string? condition, string tfm, string configuration, string platform, string path)
    {
        if (condition is null) return true;
        var parsed = ParseCondition(condition, path);
        var actual = parsed.variable switch { "TargetFramework" => tfm, "Configuration" => configuration, _ => platform };
        var equal = actual == parsed.literal;
        return parsed.operation == "==" ? equal : !equal;
    }

    /// <summary>只解析已列举属性的单一字面比较，不求值任意 MSBuild 表达式。</summary>
    private static (string variable, string operation, string literal) ParseCondition(string condition, string path)
    {
        var match = System.Text.RegularExpressions.Regex.Match(condition,
            "^\\s*['\"]\\$\\((TargetFramework|Configuration|Platform)\\)['\"]\\s*(==|!=)\\s*['\"]([^'\"]+)['\"]\\s*$", System.Text.RegularExpressions.RegexOptions.CultureInvariant);
        if (!match.Success) throw new HelperException("unsupported_configuration", "Unsupported Compile condition", path);
        return (match.Groups[1].Value, match.Groups[2].Value, match.Groups[3].Value);
    }

    /// <summary>将版本限制在已经固定验证的 C# 14 语法。</summary>
    private static LanguageVersion ParseLanguage(string version)
    {
        if (version is not ("14" or "14.0"))
            throw new HelperException("unsupported_configuration", $"Unsupported C# language version {version}");
        return LanguageVersion.CSharp14;
    }

    /// <summary>校验源码确实位于仓库中且不是越界符号链接。</summary>
    public static string CheckFile(string root, string relative)
    {
        if (string.IsNullOrWhiteSpace(relative) || Path.IsPathRooted(relative) || relative.Contains('\\') ||
            relative.Split('/').Any(part => part is "" or "." or "..") || !relative.EndsWith(".cs", StringComparison.OrdinalIgnoreCase))
            throw new HelperException("invalid_input", "Invalid C# source path", relative);
        var file = Path.GetFullPath(Path.Combine(root, relative));
        if (!file.StartsWith(root + Path.DirectorySeparatorChar, StringComparison.Ordinal) || !File.Exists(file))
            throw new HelperException("invalid_input", "C# source missing or outside repository", relative);
        CheckRepositoryPath(root, file);
        return file;
    }

    /// <summary>以不跟随目录链接的方式枚举受控源码或项目文件。</summary>
    private static IEnumerable<string> SafeFiles(string root, string extension, bool excludeHiddenDirectories = false)
    {
        var pending = new Stack<string>();
        pending.Push(root);
        while (pending.Count > 0)
        {
            var directory = pending.Pop();
            foreach (var entry in Directory.EnumerateFileSystemEntries(directory))
            {
                var attributes = File.GetAttributes(entry);
                if (attributes.HasFlag(FileAttributes.Directory))
                {
                    var name = Path.GetFileName(entry);
                    if (name is "bin" or "obj" or ".git" ||
                        (excludeHiddenDirectories && name.StartsWith(".", StringComparison.Ordinal))) continue;
                    if (attributes.HasFlag(FileAttributes.ReparsePoint))
                        throw new HelperException("unsupported_configuration", "Symbolic-link directory is unsupported",
                            Path.GetRelativePath(root, entry).Replace('\\', '/'));
                    pending.Push(entry);
                }
                else if (Path.GetExtension(entry).Equals(extension, StringComparison.OrdinalIgnoreCase))
                {
                    if (attributes.HasFlag(FileAttributes.ReparsePoint))
                        throw new HelperException("unsupported_configuration", "Symbolic-link project/source is unsupported",
                            Path.GetRelativePath(root, entry).Replace('\\', '/'));
                    yield return entry;
                }
            }
        }
    }

    /// <summary>拒绝仓内源文件、项目和配置的任何符号链接路径段。</summary>
    private static void CheckRepositoryPath(string root, string path)
    {
        for (var cursor = path; cursor.Length >= root.Length; cursor = Path.GetDirectoryName(cursor)!)
        {
            if (File.GetAttributes(cursor).HasFlag(FileAttributes.ReparsePoint))
                throw new HelperException("invalid_input", "Symbolic-link input is unsupported",
                    Path.GetRelativePath(root, path).Replace('\\', '/'));
            if (cursor == root) break;
        }
    }

    /// <summary>拒绝声明式模型中的重复与额外字段。</summary>
    private static void ExactFields(JsonElement element, params string[] fields)
    {
        if (element.ValueKind != JsonValueKind.Object || element.EnumerateObject().Count() != fields.Length ||
            element.EnumerateObject().Select(p => p.Name).Distinct(StringComparer.Ordinal).Count() != fields.Length ||
            element.EnumerateObject().Any(p => !fields.Contains(p.Name, StringComparer.Ordinal)))
            throw new HelperException("unsupported_configuration", "Invalid source-set fields");
    }
}
