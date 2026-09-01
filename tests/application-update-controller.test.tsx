import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { ApplicationUpdateApplication, ApplicationUpdateSnapshot } from "@/application-update/application-update";
import { useApplicationUpdate } from "@/application-update/use-application-update";
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
    <ApplicationUpdateSettings update={update} />
  </>;
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
});
