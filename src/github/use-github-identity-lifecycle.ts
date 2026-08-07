import { useCallback, useEffect, useRef, useState } from "react";

import { applicationError, type ApplicationError } from "@/app/application-error";
import type {
  GitHubAuthStatus,
  GitHubOAuthApplicationAdapter,
  DeviceAuthorization,
} from "@/github/github-oauth-application";

/** GitHub Identity lifecycle 当前正在执行的转换。 */
export type GitHubIdentityTransition =
  | { kind: "loading" }
  | { kind: "refreshing-identity" }
  | { kind: "starting-authorization" }
  | { kind: "completing-authorization" }
  | { kind: "signing-out" }
  | { kind: "stable" };

/** GitHub Identity 用户操作的稳定结果。 */
export type GitHubIdentityActionResult =
  | { kind: "authorized" }
  | { kind: "failed" };

/** GitHub Identity lifecycle 的启用选项。 */
export type GitHubIdentityLifecycleOptions = {
  enabled: boolean;
};

/** GitHub Identity lifecycle 暴露给调用方的稳定状态。 */
export type GitHubIdentityLifecycleBinding = {
  status?: GitHubAuthStatus;
  authorization?: DeviceAuthorization;
  error?: ApplicationError;
  transition: GitHubIdentityTransition;
  beginAuthorization(): Promise<
    | { kind: "started"; authorization: DeviceAuthorization }
    | { kind: "failed" }
  >;
  completeAuthorization(): Promise<GitHubIdentityActionResult>;
  signOut(): Promise<{ kind: "signed-out" } | { kind: "failed" }>;
  retry(): Promise<void>;
};

/** 先发布 cached GitHub Identity，再独立刷新公开身份。 */
export function useGitHubIdentityLifecycle(
  adapter: GitHubOAuthApplicationAdapter,
  options: GitHubIdentityLifecycleOptions,
): GitHubIdentityLifecycleBinding {
  const [status, setStatus] = useState<GitHubAuthStatus>();
  const [authorization, setAuthorization] = useState<DeviceAuthorization>();
  const [error, setError] = useState<ApplicationError>();
  const [transition, setTransition] = useState<GitHubIdentityTransition>(
    options.enabled ? { kind: "loading" } : { kind: "stable" },
  );
  const requestRef = useRef(0);

  /** 读取 cached authorization status，并在已登录时后台刷新公开身份。 */
  const load = useCallback(async () => {
    const request = ++requestRef.current;
    setError(undefined);
    setTransition({ kind: "loading" });
    try {
      const loaded = await adapter.loadStatus();
      if (request !== requestRef.current) return;
      setStatus(loaded);
      if (!loaded.signedIn) return;
      setTransition({ kind: "refreshing-identity" });
      try {
        const identity = await adapter.refreshIdentity();
        if (request === requestRef.current) {
          setStatus((value) => value ? { ...value, identity } : value);
        }
      } catch {
        // 公开身份刷新失败不改变 cached identity 或有效登录状态。
      }
    } catch (reason) {
      if (request === requestRef.current) {
        setError(applicationError("github-identity", "load-status", reason));
      }
    } finally {
      if (request === requestRef.current) setTransition({ kind: "stable" });
    }
  }, [adapter]);

  useEffect(() => {
    if (!options.enabled) return;
    void load();
    return () => {
      requestRef.current += 1;
    };
  }, [load, options.enabled]);

  /** 开始一次由用户明确触发的 Device Flow。 */
  const beginAuthorization = useCallback(async () => {
    const request = ++requestRef.current;
    setError(undefined);
    setTransition({ kind: "starting-authorization" });
    try {
      const started = await adapter.beginAuthorization();
      if (request !== requestRef.current) return { kind: "failed" } as const;
      setAuthorization(started);
      return { kind: "started", authorization: started } as const;
    } catch (reason) {
      if (request === requestRef.current) {
        setError(applicationError("github-identity", "begin-authorization", reason));
      }
      return { kind: "failed" } as const;
    } finally {
      if (request === requestRef.current) setTransition({ kind: "stable" });
    }
  }, [adapter]);

  /** 完成当前 Device Flow，并立即提交后端确认的 GitHub Identity。 */
  const completeAuthorization = useCallback(async (): Promise<GitHubIdentityActionResult> => {
    if (!authorization) return { kind: "failed" };
    const request = ++requestRef.current;
    setError(undefined);
    setTransition({ kind: "completing-authorization" });
    try {
      const completed = await adapter.completeAuthorization(authorization.authorizationId);
      if (request !== requestRef.current) return { kind: "failed" };
      setStatus(completed);
      setAuthorization(undefined);
      return { kind: "authorized" };
    } catch (reason) {
      if (request === requestRef.current) {
        setError(applicationError("github-identity", "complete-authorization", reason));
      }
      return { kind: "failed" };
    } finally {
      if (request === requestRef.current) setTransition({ kind: "stable" });
    }
  }, [adapter, authorization]);

  /** 退出 GitHub，并在失败时保留后端仍确认有效的身份。 */
  const signOut = useCallback(async () => {
    const request = ++requestRef.current;
    setError(undefined);
    setTransition({ kind: "signing-out" });
    try {
      const result = await adapter.signOut();
      if (request !== requestRef.current) return { kind: "failed" } as const;
      if (result.status) setStatus(result.status);
      if (result.kind === "failed") {
        setError(applicationError("github-identity", "sign-out", result.detail));
        return { kind: "failed" } as const;
      }
      setAuthorization(undefined);
      return { kind: "signed-out" } as const;
    } catch (reason) {
      if (request === requestRef.current) {
        setError(applicationError("github-identity", "sign-out", reason));
      }
      return { kind: "failed" } as const;
    } finally {
      if (request === requestRef.current) setTransition({ kind: "stable" });
    }
  }, [adapter]);

  return {
    status,
    authorization,
    error,
    transition,
    beginAuthorization,
    completeAuthorization,
    signOut,
    retry: load,
  };
}
