using System.Text;
using System.Text.Json;

namespace Narrant.CSharp.Helper;

/// <summary>运行一次 v1 NDJSON 会话并在终结消息后退出。</summary>
internal static class Program
{
    /// <summary>启动 helper，后台读取取消以便 Roslyn 阶段及时停止。</summary>
    private static async Task<int> Main()
    {
        Console.InputEncoding = Encoding.UTF8;
        Console.OutputEncoding = new UTF8Encoding(false);
        await Console.Out.WriteLineAsync(Protocol.Serialize(new
        {
            protocol = Protocol.Version, type = "ready", helper_id = "narrant-csharp",
            helper_version = "0.1.0", projection_revision = "csharp-source-v1",
            languages = new[] { "csharp" }, profiles = new[] { "csharp-static-v1" }
        }));
        await Console.Out.FlushAsync();
        string? line = await Console.In.ReadLineAsync();
        if (line is null) return 1;
        AnalyzeRequest request;
        try { request = Protocol.ParseAnalyze(line); }
        catch (Exception error)
        {
            Console.Error.WriteLine($"Invalid analyze message: {error.Message}");
            return 1;
        }

        using var cancellation = new CancellationTokenSource();
        using var deadline = new CancellationTokenSource(request.Limits.AnalysisTimeoutMs);
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellation.Token, deadline.Token);
        var cancelledByHost = false;
        string? invalidControl = null;
        _ = Task.Run(async () =>
        {
            string? next = await Console.In.ReadLineAsync();
            if (next is null) { cancellation.Cancel(); return; }
            try
            {
                if (Protocol.ParseCancel(next) == request.Id) cancelledByHost = true;
                else invalidControl = "Cancel id does not match active request";
            }
            catch (Exception error) { invalidControl = $"Invalid control message: {error.Message}"; }
            cancellation.Cancel();
        });

        object result;
        try
        {
            var output = await Task.Run(() => Analyzer.Analyze(request, linked.Token), linked.Token);
            result = new { protocol = Protocol.Version, type = "completed", id = request.Id,
                outcome = "ok", graph = output.Graph, evidence = output.Evidence };
        }
        catch (OperationCanceledException)
        {
            result = invalidControl is not null
                ? new { protocol = Protocol.Version, type = "completed", id = request.Id,
                    outcome = "error", error = new { code = "invalid_input", message = invalidControl, paths = Array.Empty<string>() } }
                : !deadline.IsCancellationRequested && cancelledByHost
                ? new { protocol = Protocol.Version, type = "completed", id = request.Id, outcome = "cancelled" }
                : new { protocol = Protocol.Version, type = "completed", id = request.Id,
                    outcome = "error", error = new { code = "resource_limit", message = "Analysis timed out or input closed", paths = Array.Empty<string>() } };
        }
        catch (HelperException error)
        {
            result = new { protocol = Protocol.Version, type = "completed", id = request.Id,
                outcome = "error", error = new { code = error.Code, message = error.Message, paths = error.Paths } };
        }
        catch (Exception error)
        {
            result = new { protocol = Protocol.Version, type = "completed", id = request.Id,
                outcome = "error", error = new { code = "internal_failure", message = error.Message, paths = Array.Empty<string>() } };
        }
        var serialized = Protocol.Serialize(result);
        if (Encoding.UTF8.GetByteCount(serialized) > request.Limits.MaxResultBytes)
            serialized = Protocol.Serialize(new { protocol = Protocol.Version, type = "completed", id = request.Id,
                outcome = "error", error = new { code = "resource_limit", message = "Result exceeds byte limit", paths = Array.Empty<string>() } });
        if (Encoding.UTF8.GetByteCount(serialized) > request.Limits.MaxResultBytes)
        {
            Console.Error.WriteLine("Result byte limit is smaller than a completed error message");
            return 1;
        }
        await Console.Out.WriteLineAsync(serialized);
        await Console.Out.FlushAsync();
        return 0;
    }
}
