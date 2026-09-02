import { StrictMode, useEffect } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { ApplicationUpdateApplication, ApplicationUpdateSnapshot } from "@/application-update/application-update";
import { useApplicationUpdate } from "@/application-update/use-application-update";
import { ApplicationUpdateSurface } from "@/components/application-update/application-update-surface";
import { ApplicationUpdateSettings } from "@/components/settings-page/application-update-settings";
import "@/lib/i18n";

afterEach(cleanup);

/** 构造只含前端允许观察字段的快照。 */
function snapshot(sequence: number, automaticDownload = true): ApplicationUpdateSnapshot {
  return {
    sequence,
    currentVersion: "2.0.0",
    automaticDownload,
    state: { kind: "up_to_date" },
    candidate: null,
    failure: null,
    recoveryWarning: null,
    preferenceWarning: null,
  };
}

/** 创建可手动发布单一 snapshot event 的测试端口。 */
function application(initial = snapshot(1)) {
  let listener: ((value: ApplicationUpdateSnapshot) => void) | undefined;
  const port: ApplicationUpdateApplication = {
    snapshot: vi.fn(async () => initial),
    subscribe: vi.fn(async (next) => { listener = next; return () => { listener = undefined; }; }),
    manualCheck: vi.fn(async () => initial),
    setAutomaticDownload: vi.fn(async (enabled) => ({ ...initial, sequence: initial.sequence + 1, automaticDownload: enabled })),
    retry: vi.fn(async () => initial),
    restart: vi.fn(async () => initial),
  };
  return { port, emit: (value: ApplicationUpdateSnapshot) => listener?.(value) };
}

function Harness({ application: port }: { application: ApplicationUpdateApplication }) {
  const update = useApplicationUpdate(port);
  return <>
    <output aria-label="sequence">{update.snapshot?.sequence}</output>
    <output aria-label="pending">{update.pending}</output>
    <output aria-label="command error">{update.commandError}</output>
    <button onClick={() => void update.setAutomaticDownload(false)}>force off</button>
    <button onClick={() => void update.setAutomaticDownload(true)}>force on</button>
    <ApplicationUpdateSettings update={update} />
  </>;
}

/** 为异步命令测试提供可控的完成顺序。 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolveValue, rejectValue) => { resolve = resolveValue; reject = rejectValue; });
  return { promise, resolve, reject };
}

/** 自动打开真实更新弹层，以验证 controller 与 surface 的错误协作。 */
function SurfaceHarness({ application: port }: { application: ApplicationUpdateApplication }) {
  const update = useApplicationUpdate(port);
  const updateSnapshot = update.snapshot;
  const openSurface = update.openSurface;
  useEffect(() => {
    if (updateSnapshot) openSurface();
  }, [openSurface, updateSnapshot]);
  return <ApplicationUpdateSurface update={update} />;
}

describe("Application Update controller", () => {
  it("先监听再读取初始快照，并丢弃 stale 与 duplicate sequence", async () => {
    let resolveInitial!: (value: ApplicationUpdateSnapshot) => void;
    const fixture = application();
    vi.mocked(fixture.port.snapshot).mockReturnValue(new Promise((resolve) => { resolveInitial = resolve; }));
    render(<Harness application={fixture.port} />);

    await waitFor(() => expect(fixture.port.subscribe).toHaveBeenCalledOnce());
    fixture.emit(snapshot(4, false));
    resolveInitial(snapshot(3, true));
    await waitFor(() => expect(screen.getByLabelText("sequence")).toHaveTextContent("4"));
    expect(screen.getByRole("switch", { name: "自动下载更新" })).not.toBeChecked();

    fixture.emit(snapshot(4, true));
    fixture.emit(snapshot(2, true));
    expect(screen.getByRole("switch", { name: "自动下载更新" })).not.toBeChecked();
  });

  it("偏好命令 IPC 失败时回滚乐观值并显示稳定警告", async () => {
    const fixture = application(snapshot(1, true));
    vi.mocked(fixture.port.setAutomaticDownload).mockRejectedValue(new Error("private adapter detail"));
    render(<Harness application={fixture.port} />);
    const toggle = await screen.findByRole("switch", { name: "自动下载更新" });

    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());
    expect(await screen.findByRole("alert")).toHaveTextContent("自动下载偏好未能保存");
    expect(screen.queryByText(/private adapter detail/)).not.toBeInTheDocument();
  });

  it("偏好 durability warning 使用命令返回的权威值而不回滚", async () => {
    const fixture = application(snapshot(1, true));
    vi.mocked(fixture.port.setAutomaticDownload).mockResolvedValue({
      ...snapshot(2, false),
      preferenceWarning: { kind: "durability_unknown", category: "storage", message: "backend localized text" },
    });
    render(<Harness application={fixture.port} />);
    const toggle = await screen.findByRole("switch", { name: "自动下载更新" });

    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(screen.getByText(/持久保存尚未确认/)).toHaveAttribute("role", "status");
  });

  it("偏好 reject 错误会被较晚的权威 warning event 清理", async () => {
    const fixture = application(snapshot(1, true));
    vi.mocked(fixture.port.setAutomaticDownload).mockRejectedValue(new Error("rejected"));
    render(<Harness application={fixture.port} />);
    const toggle = await screen.findByRole("switch", { name: "自动下载更新" });

    fireEvent.click(toggle);
    await waitFor(() => expect(screen.getByLabelText("command error")).toHaveTextContent("preference"));
    act(() => fixture.emit({
      ...snapshot(2, false),
      preferenceWarning: { kind: "durability_unknown", category: "storage", message: "event" },
    }));

    expect(toggle).not.toBeChecked();
    expect(screen.getByLabelText("sequence")).toHaveTextContent("2");
    expect(screen.getByLabelText("command error")).toBeEmptyDOMElement();
    expect(screen.getByText(/持久保存尚未确认/)).toHaveAttribute("role", "status");
  });

  it("权威事件先到而偏好 IPC 后拒绝时不回滚也不留下过时错误", async () => {
    const fixture = application(snapshot(1, true));
    const command = deferred<ApplicationUpdateSnapshot>();
    vi.mocked(fixture.port.setAutomaticDownload).mockReturnValue(command.promise);
    render(<Harness application={fixture.port} />);
    const toggle = await screen.findByRole("switch", { name: "自动下载更新" });

    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).not.toBeChecked());
    act(() => fixture.emit({
      ...snapshot(2, true),
      preferenceWarning: { kind: "durability_unknown", category: "storage", message: "redacted" },
    }));
    await act(async () => { command.reject(new Error("late reject")); });

    expect(toggle).toBeChecked();
    expect(screen.getByText(/持久保存尚未确认/)).toHaveAttribute("role", "status");
    expect(screen.queryByText(/未能保存，已恢复/)).not.toBeInTheDocument();
  });

  it("权威事件先到后忽略偏好命令的较晚旧 resolve", async () => {
    const fixture = application(snapshot(1, true));
    const command = deferred<ApplicationUpdateSnapshot>();
    vi.mocked(fixture.port.setAutomaticDownload).mockReturnValue(command.promise);
    render(<Harness application={fixture.port} />);
    const toggle = await screen.findByRole("switch", { name: "自动下载更新" });

    fireEvent.click(toggle);
    act(() => fixture.emit(snapshot(3, true)));
    await act(async () => command.resolve(snapshot(2, false)));

    expect(toggle).toBeChecked();
    expect(screen.getByLabelText("sequence")).toHaveTextContent("3");
    expect(screen.queryByText(/未能保存/)).not.toBeInTheDocument();
  });

  it("A(false) 与 B(true) 都 reject 时回滚到唯一权威值而非另一个乐观值", async () => {
    const fixture = application(snapshot(1, true));
    const first = deferred<ApplicationUpdateSnapshot>();
    const second = deferred<ApplicationUpdateSnapshot>();
    vi.mocked(fixture.port.setAutomaticDownload)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    render(<Harness application={fixture.port} />);
    await screen.findByRole("switch", { name: "自动下载更新" });

    fireEvent.click(screen.getByRole("button", { name: "force off" }));
    fireEvent.click(screen.getByRole("button", { name: "force on" }));
    expect(fixture.port.setAutomaticDownload).toHaveBeenNthCalledWith(1, false);
    expect(fixture.port.setAutomaticDownload).toHaveBeenNthCalledWith(2, true);
    expect(screen.getByLabelText("pending")).toHaveTextContent("preference");
    await act(async () => second.reject(new Error("B rejected")));
    await act(async () => first.reject(new Error("A rejected")));

    expect(screen.getByRole("switch", { name: "自动下载更新" })).toBeChecked();
    expect(screen.getByLabelText("sequence")).toHaveTextContent("1");
    expect(screen.getByLabelText("pending")).toBeEmptyDOMElement();
    expect(screen.getByLabelText("command error")).toHaveTextContent("preference");
  });

  it("当前 B reject 后旧请求 A 的更高 sequence 成功会清理错误并成为权威", async () => {
    const fixture = application(snapshot(1, true));
    const first = deferred<ApplicationUpdateSnapshot>();
    const second = deferred<ApplicationUpdateSnapshot>();
    vi.mocked(fixture.port.setAutomaticDownload)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    render(<Harness application={fixture.port} />);
    await screen.findByRole("switch", { name: "自动下载更新" });

    fireEvent.click(screen.getByRole("button", { name: "force off" }));
    fireEvent.click(screen.getByRole("button", { name: "force on" }));
    await act(async () => second.reject(new Error("B rejected")));
    expect(screen.getByLabelText("command error")).toHaveTextContent("preference");

    await act(async () => first.resolve({
      ...snapshot(5, false),
      preferenceWarning: { kind: "state_unknown", category: "storage", message: "A" },
    }));
    expect(screen.getByRole("switch", { name: "自动下载更新" })).not.toBeChecked();
    expect(screen.getByLabelText("sequence")).toHaveTextContent("5");
    expect(screen.getByLabelText("pending")).toBeEmptyDOMElement();
    expect(screen.getByLabelText("command error")).toBeEmptyDOMElement();
    expect(screen.getByText(/无法确认自动下载偏好的当前值/)).toHaveAttribute("role", "status");
  });

  it("B sequence 4 先成功而旧 token A sequence 5 后成功时仍接受最新权威 sequence", async () => {
    const fixture = application(snapshot(1, true));
    const first = deferred<ApplicationUpdateSnapshot>();
    const second = deferred<ApplicationUpdateSnapshot>();
    vi.mocked(fixture.port.setAutomaticDownload)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    render(<Harness application={fixture.port} />);
    await screen.findByRole("switch", { name: "自动下载更新" });

    fireEvent.click(screen.getByRole("button", { name: "force off" }));
    fireEvent.click(screen.getByRole("button", { name: "force on" }));
    await act(async () => second.resolve(snapshot(4, true)));
    expect(screen.getByLabelText("sequence")).toHaveTextContent("4");
    expect(screen.getByRole("switch", { name: "自动下载更新" })).toBeChecked();
    expect(screen.getByLabelText("pending")).toBeEmptyDOMElement();

    await act(async () => first.resolve(snapshot(5, false)));
    expect(screen.getByLabelText("sequence")).toHaveTextContent("5");
    expect(screen.getByRole("switch", { name: "自动下载更新" })).not.toBeChecked();
    expect(screen.getByLabelText("pending")).toBeEmptyDOMElement();
    expect(screen.getByLabelText("command error")).toBeEmptyDOMElement();
  });

  it("双 reject 期间插入权威 warning event 时显示意图，settle 后回到该权威值", async () => {
    const fixture = application(snapshot(1, true));
    const first = deferred<ApplicationUpdateSnapshot>();
    const second = deferred<ApplicationUpdateSnapshot>();
    vi.mocked(fixture.port.setAutomaticDownload)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    render(<Harness application={fixture.port} />);
    await screen.findByRole("switch", { name: "自动下载更新" });

    fireEvent.click(screen.getByRole("button", { name: "force off" }));
    fireEvent.click(screen.getByRole("button", { name: "force on" }));
    act(() => fixture.emit({
      ...snapshot(2, false),
      preferenceWarning: { kind: "durability_unknown", category: "storage", message: "redacted" },
    }));
    expect(screen.getByRole("switch", { name: "自动下载更新" })).toBeChecked();
    expect(screen.getByLabelText("sequence")).toHaveTextContent("2");
    expect(screen.getByLabelText("pending")).toHaveTextContent("preference");
    expect(screen.getByText(/持久保存尚未确认/)).toHaveAttribute("role", "status");

    await act(async () => second.reject(new Error("B rejected")));
    await act(async () => first.reject(new Error("A rejected")));
    expect(screen.getByRole("switch", { name: "自动下载更新" })).not.toBeChecked();
    expect(screen.getByLabelText("sequence")).toHaveTextContent("2");
    expect(screen.getByLabelText("pending")).toBeEmptyDOMElement();
    expect(screen.getByLabelText("command error")).toBeEmptyDOMElement();
    expect(screen.getByText(/持久保存尚未确认/)).toHaveAttribute("role", "status");
  });

  it("event/warning 与 B4/A5 交错时逐 sequence 收敛并保留各自权威 warning", async () => {
    const fixture = application(snapshot(1, true));
    const first = deferred<ApplicationUpdateSnapshot>();
    const second = deferred<ApplicationUpdateSnapshot>();
    vi.mocked(fixture.port.setAutomaticDownload)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    render(<Harness application={fixture.port} />);
    await screen.findByRole("switch", { name: "自动下载更新" });

    fireEvent.click(screen.getByRole("button", { name: "force off" }));
    fireEvent.click(screen.getByRole("button", { name: "force on" }));
    act(() => fixture.emit({
      ...snapshot(2, false),
      preferenceWarning: { kind: "durability_unknown", category: "storage", message: "event" },
    }));
    expect(screen.getByRole("switch", { name: "自动下载更新" })).toBeChecked();
    expect(screen.getByText(/持久保存尚未确认/)).toBeVisible();

    await act(async () => second.resolve({
      ...snapshot(4, true),
      preferenceWarning: { kind: "durability_unknown", category: "storage", message: "B" },
    }));
    expect(screen.getByLabelText("sequence")).toHaveTextContent("4");
    expect(screen.getByRole("switch", { name: "自动下载更新" })).toBeChecked();
    expect(screen.getByLabelText("pending")).toBeEmptyDOMElement();
    expect(screen.getByText(/持久保存尚未确认/)).toBeVisible();

    await act(async () => first.resolve({
      ...snapshot(5, false),
      preferenceWarning: { kind: "state_unknown", category: "storage", message: "A" },
    }));
    expect(screen.getByLabelText("sequence")).toHaveTextContent("5");
    expect(screen.getByRole("switch", { name: "自动下载更新" })).not.toBeChecked();
    expect(screen.getByLabelText("pending")).toBeEmptyDOMElement();
    expect(screen.getByLabelText("command error")).toBeEmptyDOMElement();
    expect(screen.getByText(/无法确认自动下载偏好的当前值/)).toHaveAttribute("role", "status");
  });

  it("订阅失败后可显式重新连接并成功读取快照", async () => {
    const fixture = application(snapshot(1));
    const stop = vi.fn();
    vi.mocked(fixture.port.subscribe)
      .mockRejectedValueOnce(new Error("listen failed"))
      .mockResolvedValueOnce(stop);
    render(<Harness application={fixture.port} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("无法读取应用更新状态");
    fireEvent.click(screen.getByRole("button", { name: "重新连接" }));
    expect(await screen.findByRole("switch", { name: "自动下载更新" })).toBeChecked();
    expect(fixture.port.subscribe).toHaveBeenCalledTimes(2);
  });

  it("快照读取失败后 reload 先清理已有 listener 再重新订阅", async () => {
    const fixture = application(snapshot(2));
    const firstStop = vi.fn();
    const secondStop = vi.fn();
    vi.mocked(fixture.port.subscribe)
      .mockResolvedValueOnce(firstStop)
      .mockResolvedValueOnce(secondStop);
    vi.mocked(fixture.port.snapshot)
      .mockRejectedValueOnce(new Error("snapshot failed"))
      .mockResolvedValueOnce(snapshot(2));
    const view = render(<Harness application={fixture.port} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("无法读取应用更新状态");
    fireEvent.click(screen.getByRole("button", { name: "重新连接" }));
    expect(await screen.findByRole("switch", { name: "自动下载更新" })).toBeChecked();
    expect(firstStop).toHaveBeenCalledOnce();
    view.unmount();
    expect(secondStop).toHaveBeenCalledOnce();
  });

  it("StrictMode 重复 mount/unmount 不保留双 listener 或 stale async 回写", async () => {
    const active = new Set<(value: ApplicationUpdateSnapshot) => void>();
    const stops: Array<ReturnType<typeof vi.fn>> = [];
    const fixture = application(snapshot(1));
    vi.mocked(fixture.port.subscribe).mockImplementation(async (listener) => {
      active.add(listener);
      const stop = vi.fn(() => active.delete(listener));
      stops.push(stop);
      return stop;
    });
    const view = render(<StrictMode><Harness application={fixture.port} /></StrictMode>);

    await waitFor(() => expect(screen.getByLabelText("sequence")).toHaveTextContent("1"));
    await waitFor(() => expect(active.size).toBe(1));
    expect(stops.some((stop) => stop.mock.calls.length === 1)).toBe(true);
    view.unmount();
    expect(active.size).toBe(0);
    expect(stops.every((stop) => stop.mock.calls.length === 1)).toBe(true);
  });

  it("Retry reject 在弹层内提示、恢复按钮并在下一次 action 清理旧错误", async () => {
    const value: ApplicationUpdateSnapshot = {
      ...snapshot(1),
      state: { kind: "failed" },
      candidate: { version: "2.1.0", releaseNotes: "fixes" },
      failure: { operation: "download", category: "network", retryable: true, message: "redacted" },
    };
    const fixture = application(value);
    const first = deferred<ApplicationUpdateSnapshot>();
    const second = deferred<ApplicationUpdateSnapshot>();
    vi.mocked(fixture.port.retry).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    render(<SurfaceHarness application={fixture.port} />);
    const retry = await screen.findByRole("button", { name: "重试" });
    retry.focus();
    fireEvent.click(retry);
    expect(retry).toBeDisabled();
    await act(async () => first.reject(new Error("private")));

    expect(await screen.findByText("无法重试更新，请稍后再试。")).toHaveAttribute("role", "alert");
    expect(retry).toBeEnabled();
    expect(retry).toHaveFocus();
    fireEvent.click(retry);
    expect(screen.queryByText("无法重试更新，请稍后再试。")).not.toBeInTheDocument();
    expect(retry).toBeDisabled();
    expect(fixture.port.retry).toHaveBeenNthCalledWith(1);
    expect(fixture.port.retry).toHaveBeenNthCalledWith(2);
    await act(async () => second.resolve(snapshot(2)));
  });

  it("Restart reject 在弹层内提示并恢复 staged 操作", async () => {
    const value: ApplicationUpdateSnapshot = {
      ...snapshot(1),
      state: { kind: "staged" },
      candidate: { version: "2.1.0", releaseNotes: "fixes" },
    };
    const fixture = application(value);
    const restart = deferred<ApplicationUpdateSnapshot>();
    vi.mocked(fixture.port.restart).mockReturnValue(restart.promise);
    render(<SurfaceHarness application={fixture.port} />);
    const button = await screen.findByRole("button", { name: "立即重启并更新" });
    button.focus();
    fireEvent.click(button);
    expect(button).toBeDisabled();
    await act(async () => restart.reject(new Error("private")));

    expect(await screen.findByText("无法请求重启更新，请稍后再试。")).toHaveAttribute("aria-live", "assertive");
    expect(button).toBeEnabled();
    expect(button).toHaveFocus();
    expect(fixture.port.restart).toHaveBeenCalledWith();
  });
});
