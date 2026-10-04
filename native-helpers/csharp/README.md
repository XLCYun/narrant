# Narrant C# native helper

This helper implements `narrant.native-analyzer/1` for `csharp-static-v1` with
`helper_id=narrant-csharp`, `helper_version=0.1.0`, and
`projection_revision=csharp-source-v1`. It reads one UTF-8 NDJSON `analyze` request
after `ready`, accepts a matching `cancel`, writes one `completed` response, and exits.

## Fixed tool and discovered targets

Build with the official .NET SDK **10.0.100**. `global.json` disables SDK roll-forward.
The project references `Microsoft.CodeAnalysis.dll` and
`Microsoft.CodeAnalysis.CSharp.dll` from that SDK's `Roslyn/bincore`; it has no
NuGet package dependency. A framework-dependent publish contains the helper,
Roslyn DLLs, `.deps.json`, and `.runtimeconfig.json`. The support pack supplies
its own .NET runtime **10.0.0**. The helper's runtime is distinct from the
analyzed code's approved target reference assemblies.

On an isolated SDK installation:

```sh
DOTNET_ROOT=/absolute/dotnet-10.0.100 \
  /absolute/dotnet-10.0.100/dotnet build native-helpers/csharp/Narrant.CSharp.Helper.csproj -c Release
DOTNET_ROOT=/absolute/dotnet-10.0.100 \
  /absolute/dotnet-10.0.100/dotnet publish native-helpers/csharp/Narrant.CSharp.Helper.csproj \
  -c Release --no-restore -p:UseAppHost=false -o /tmp/narrant-csharp-publish
```

The Host launches the pack's private `bin/dotnet` (`bin/dotnet.exe` on Windows)
with `runtime/helper/Narrant.CSharp.Helper.dll` and no helper arguments. It sets
`DOTNET_ROOT` to the pack's `bin` directory and disables multilevel lookup.
The support pack does not bundle analyzed target references. The Host discovers
installed `Microsoft.NETCore.App.Ref/<stable-version>/ref/<tfm>` directories outside
the analyzed repository and chooses the highest stable patch per exact TFM. It
freezes that catalog for the before/after analysis. An absent matching local target
returns `target_unavailable`; the helper never borrows another target's references.

Platform `kind` is `dotnet-reference-assemblies`; its `root` is the approved reference
directory. The platform object has exactly `id`, `kind`, `version`, `root`, and
`metadata`; no fingerprint is required or accepted. The helper accepts canonical
modern base TFMs `netN.0` with `N >= 5`. `version` must equal
`metadata.reference_pack_version`, a stable three-part `N.0.patch` version for the
same target. The patch is not fixed in the product. For example:

```json
[
  {"tfm":"net8.0","reference_pack":"Microsoft.NETCore.App.Ref","reference_pack_version":"8.0.31"},
  {"tfm":"net9.0","reference_pack":"Microsoft.NETCore.App.Ref","reference_pack_version":"9.0.20"}
]
```

Only `.dll` files directly in that directory become `MetadataReference`s. No
runtime assembly or installed NuGet cache is used as an analyzed target.

## Source-set format

When no `.csproj` exists, the repository root may contain `narrant.csharp.json`:

```json
{
  "version": "csharp-static-v1",
  "configurations": [
    {
      "id": "app-net10",
      "files": ["src/App.cs", "src/Model.cs"],
      "tfm": "net10.0",
      "language_version": "14.0",
      "define_constants": ["FEATURE_A"],
      "implicit_usings": false,
      "test_only": false
    }
  ]
}
```

`language_version` accepts explicit numeric language versions recognized by the
bundled Roslyn, such as `12`/`12.0` or `13`/`13.0`. The nondeterministic aliases
`latest`, `latestMajor`, `default`, and `preview` are unsupported. SDK-style projects
derive the default C# major version as the modern .NET major version plus four
(net8.0 → C# 12, net9.0 → C# 13, net10.0 → C# 14), following the
[Roslyn SDK target rule](https://github.com/dotnet/roslyn/blob/main/src/Compilers/Core/MSBuildTask/Microsoft.CSharp.Core.targets).
If that default exceeds the bundled Roslyn's capabilities, configuration fails
explicitly; it does not silently use an older language. An explicit supported
numeric `LangVersion` can be used, but still requires the exact local target.
Source-set preprocessor symbols are
exactly its explicit `define_constants`; SDK-style projects receive the target's
predefined symbols, including `NET`, `NETCOREAPP`, `NETN_0`, and earlier
`NETN_0_OR_GREATER` / historical `NETCOREAPP*_OR_GREATER` symbols, following the
[SDK rules](https://learn.microsoft.com/en-us/dotnet/standard/frameworks#preprocessor-symbols).
`test_only` may be omitted and defaults to false. Each configuration is compiled
separately; the final graph unions compatible source objects and confirmed
relationships. Source files remain in place. An explicit selected file outside
all configurations fails instead of becoming an empty graph.

## Static SDK project subset

The helper also reads ordinary literal `Microsoft.NET.Sdk` `.csproj` files.
Supported properties are `TargetFramework`/`TargetFrameworks` (canonical `netN.0`,
`N >= 5`), `LangVersion` (supported numeric version), `DefineConstants`, `ImplicitUsings`,
`EnableDefaultCompileItems`, `IsTestProject`, `Configuration` (`Debug` or
`Release`), `Platform` (`AnyCPU`), `Nullable`, `OutputType`, `RootNamespace`,
`AssemblyName`, and `TreatWarningsAsErrors`. Default `**/*.cs` discovery
excludes `bin`, `obj`, and hidden directories. Literal `Compile Include`/`Remove` and
`ProjectReference Include` work. `Compile` and `ProjectReference` may carry a
literal equality/inequality condition on `TargetFramework`, `Configuration`, or
`Platform`, directly or on their `ItemGroup`. `Debug` and `AnyCPU` are defaults;
literal Debug/Release equality or inequality branches are compiled separately and unioned.
Referenced projects inherit the caller's static configuration, even if they have
no configuration condition of their own. The matching
`DEBUG`/`TRACE` and target-specific preprocessor constants are provided. `ImplicitUsings`
accepts `enable` or `true` and requires C# 10 or later for generated global usings.
Explicitly conditionally omitted files receive `inactive`
coverage. `IsTestProject=true` gives reliable `excluded_test` coverage when
`include_tests=false`.

Custom SDKs, imports, targets, package references, property functions,
arbitrary conditions, globs inside explicit `Compile`, and conditional
`PropertyGroup`s fail with `unsupported_configuration`. Razor, Unity project
models, scripts, legacy projects, .NET Framework / .NET Standard / .NET Core TFMs,
OS-specific TFMs and workload models are not supported. Unsupported language
versions fail explicitly. The helper never invokes MSBuild on the analyzed project, restores
its dependencies, runs a generator/analyzer, emits its assembly, or writes into
the repository.

## Tests

```sh
NARRANT_TEST_DOTNET=/absolute/dotnet-10.0.100/dotnet \
NARRANT_TEST_REFS=/absolute/dotnet-10.0.100/packs/Microsoft.NETCore.App.Ref/10.0.0/ref/net10.0 \
NARRANT_TEST_REFS8=/absolute/Microsoft.NETCore.App.Ref/8.0.0/ref/net8.0 \
NARRANT_TEST_REFS9=/absolute/Microsoft.NETCore.App.Ref/9.0.20/ref/net9.0 \
NARRANT_TEST_REFS9_VERSION=9.0.20 \
python3 -m unittest discover -s native-helpers/csharp/tests -v
```

The .NET 8/9/10 versions above are test fixtures, not the product's target catalog.
The optional .NET 9 fixture proves a previously unlisted TFM and a nonzero patch
with real reference assemblies; CI configures it so this regression test runs.
